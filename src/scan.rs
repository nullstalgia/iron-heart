use crate::app::{DeviceUpdate, ErrorPopup};
use crate::structs::{Characteristic, DeviceInfo};
// TODO See if this weird manager shadowing is normal
use btleplug::api::{
    Central, CentralEvent, Manager as _, Peripheral, PeripheralProperties, ScanFilter,
};
use btleplug::platform::{Adapter, Manager};
use futures::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Scans for Bluetooth devices and sends the information to the provided `mpsc::Sender`.
/// The scan can be paused by setting the `pause_signal` to `true`.
pub async fn bluetooth_event_thread(
    tx: mpsc::Sender<DeviceUpdate>,
    mut restart_signal: mpsc::Receiver<()>,
    pause_signal: Arc<AtomicBool>,
    cancel_token: CancellationToken,
) {
    info!("Bluetooth CentralEvent thread started!");
    // If no event is heard in this period,
    // the manager and adapter will be recreated
    // (if the scan isn't paused)
    let duration = Duration::from_secs(30);

    let manager = match Manager::new().await {
        Ok(manager) => manager,
        Err(e) => {
            error!("Failed to create manager: {}", e);
            tx.send(DeviceUpdate::Error(ErrorPopup::detailed(
                "Failed to create manager: ",
                e.into(),
            )))
            .await
            .expect("Failed to send error message");
            return;
        }
    };

    let central: Adapter;

    'adapter: loop {
        if cancel_token.is_cancelled() {
            info!("Shutting down Bluetooth CentralEvent thread!");
            return;
        }
        match manager.adapters().await.and_then(|adapters| {
            debug!("Found adapters: {adapters:#?}");
            adapters
                .into_iter()
                .next()
                .ok_or(btleplug::Error::DeviceNotFound)
        }) {
            Ok(adapter) => {
                central = adapter;
                break 'adapter;
            }
            Err(_) => {
                error!("No Bluetooth adapters found!");
                tx.send(DeviceUpdate::Error(ErrorPopup::UserMustDismiss(
                    "No Bluetooth adapters found! Make sure it's plugged in and enabled."
                        .to_string(),
                )))
                .await
                .expect("Failed to send error message");
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        };
    }

    if let Err(e) = central.start_scan(ScanFilter::default()).await {
        error!("Scanning failure: {e}");
        tx.send(DeviceUpdate::Error(ErrorPopup::Fatal(format!(
            "Scanning failure: {e}"
        ))))
        .await
        .expect("Failed to send error message");
        // tokio::time::sleep(Duration::from_secs(10)).await;
        return;
    }

    let mut events = match central.events().await {
        Ok(e) => e,
        Err(e) => {
            error!("BLE failure: {e}");
            tx.send(DeviceUpdate::Error(ErrorPopup::Fatal(format!(
                "BLE failure: {e}"
            ))))
            .await
            .expect("Failed to send error message");
            // tokio::time::sleep(Duration::from_secs(5)).await;
            return;
        }
    };
    info!("Inital scanning started!");
    let mut scanning = true;

    'events: loop {
        if pause_signal.load(Ordering::SeqCst) {
            if scanning {
                info!("Pausing scan");
                central.stop_scan().await.expect("Failed to stop scan!");
                scanning = false;
            }
        } else if !scanning {
            info!("Resuming scan");
            if let Err(e) = central.start_scan(ScanFilter::default()).await {
                error!("Failed to resume scanning: {e}");
                tx.send(DeviceUpdate::Error(ErrorPopup::UserMustDismiss(format!(
                    "Failed to resume scanning: {e}"
                ))))
                .await
                .expect("Failed to send error message");
                break 'events;
            }
            scanning = true;
        }

        tokio::select! {
            Some(event) = events.next() => {
                // debug!("{:?}", event);
                match event {
                    CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => {
                        if let Ok(device) = central.peripheral(&id).await {
                            let properties = device
                                .properties()
                                .await
                                .unwrap()
                                .unwrap_or(PeripheralProperties::default());

                            if properties.services.is_empty() {
                                continue 'events;
                            }

                            // Add the device's information to the discovered list
                            let device = DeviceInfo::new(
                                device.id().to_string(),
                                properties.local_name,
                                properties.tx_power_level,
                                properties.address.to_string(),
                                properties.rssi,
                                properties.manufacturer_data,
                                properties.services,
                                properties.service_data,
                                device.clone(),
                            );

                            // Send a clone of the accumulated device information so far
                            if tx.send(DeviceUpdate::DeviceInfo(device)).await.is_err() {
                                error!("Couldn't send device info update!");
                                break 'events;
                            }
                        }
                    }
                    CentralEvent::DeviceDisconnected(id) => {
                        warn!("Device disconnected: {}", id);
                        if tx.send(DeviceUpdate::DisconnectedEvent(id.to_string())).await.is_err() {
                            error!("Couldn't send DisconnectedEvent!");
                            break 'events;
                        }
                    }
                    CentralEvent::DeviceConnected(id) => {
                        info!("Device connected: {}", id);
                        if tx.send(DeviceUpdate::ConnectedEvent(id.to_string())).await.is_err() {
                            error!("Couldn't send ConnectedEvent!");
                            break 'events;
                        }
                    }
                    _ => {}
                }
            }
            _ = cancel_token.cancelled() => {
                info!("Shutting down Bluetooth CentralEvent thread!");
                break 'events;
            }
            _ = tokio::time::sleep(duration) => {
                debug!("CentralEvent timeout");
                if !pause_signal.load(Ordering::SeqCst) {
                    warn!("Restarting scan!");
                    if scanning {
                        if let Err(e) = central.stop_scan().await {
                            warn!("Error stopping scan: {e}");
                        }
                        scanning = false;
                    }
                }
            }
            Some(()) = restart_signal.recv() => {
                warn!("Got signal to restart scan from HR Notif thread!");
                // debug!("Central State was: {central:#?}");
                pause_signal.store(false, Ordering::SeqCst);
            }
        }
    }
}

/// Gets the characteristics of a Bluetooth device and returns them as a `Vec<Characteristic>`.
/// The device is identified by its address or UUID.
pub async fn get_characteristics(tx: mpsc::Sender<DeviceUpdate>, peripheral: DeviceInfo) {
    let duration = Duration::from_secs(10);
    match &peripheral.device {
        Some(device) => match timeout(duration, device.connect()).await {
            Ok(Ok(_)) => {
                if let Some(device) = &peripheral.device {
                    device.discover_services().await.unwrap();
                    let characteristics = device.characteristics();
                    let mut result = Vec::new();
                    for characteristic in characteristics {
                        result.push(Characteristic {
                            uuid: characteristic.uuid,
                            properties: characteristic.properties,
                            descriptors: characteristic
                                .descriptors
                                .into_iter()
                                .map(|d| d.uuid)
                                .collect(),
                            service: characteristic.service_uuid,
                        });
                    }
                    tx.send(DeviceUpdate::Characteristics(result))
                        .await
                        .expect("Failed to send error message");
                }
            }
            Ok(Err(e)) => {
                error!("Characteristics: connection error: {e}");
                tx.send(DeviceUpdate::Error(ErrorPopup::Intermittent(format!(
                    "Connection error: {e}"
                ))))
                .await
                .expect("Failed to send error message");
            }
            Err(_) => {
                error!("Characteristics: connection timed out");
                tx.send(DeviceUpdate::Error(ErrorPopup::Intermittent(
                    "Connection timed out".to_string(),
                )))
                .await
                .expect("Failed to send error message");
            }
        },
        None => {
            error!("Characteristics: device not found");
            tx.send(DeviceUpdate::Error(ErrorPopup::Fatal(
                "Device not found".to_string(),
            )))
            .await
            .expect("Failed to send error message");
        }
    }
}
