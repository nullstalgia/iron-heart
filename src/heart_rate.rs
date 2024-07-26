use crate::app::{DeviceData, ErrorPopup};
use crate::heart_rate_measurement::{parse_hrm, HeartRateMeasurement};
use crate::structs::{Characteristic, DeviceInfo};
// TODO See if this weird manager shadowing is normal
use btleplug::api::{
    Central, CentralEvent, Manager as _, Peripheral, PeripheralProperties, ScanFilter,
};
use btleplug::platform::Manager;
use futures::StreamExt;
use log::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::timeout;
use uuid::Uuid;

pub const HEART_RATE_SERVICE_UUID: Uuid = Uuid::from_u128(0x0000180d_0000_1000_8000_00805f9b34fb);

pub const HEART_RATE_MEASUREMENT_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0x00002a37_0000_1000_8000_00805f9b34fb);

pub const BATTERY_LEVEL_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0x00002a19_0000_1000_8000_00805f9b34fb);
pub const BATTERY_SERVICE_UUID: Uuid = Uuid::from_u128(0x0000180f_0000_1000_8000_00805f9b34fb);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BatteryLevel {
    #[default]
    Unknown,
    NotReported,
    Level(u8),
}

#[derive(Debug, Clone, Default)]
pub struct HeartRateStatus {
    pub heart_rate_bpm: u16,
    pub rr_intervals: Vec<f32>,
    pub battery_level: BatteryLevel,
}

// #[derive(Error, Debug)]
// pub enum MonitorError {
//     #[error("Device is missing HR service")]
//     BLEError(#[from] btleplug::Error),
// }

//pub async fn subscribe_to_heart_rate

pub async fn start_notification_thread(
    hr_tx: mpsc::UnboundedSender<DeviceData>,
    peripheral: Arc<DeviceInfo>,
) {
    let duration = Duration::from_secs(30);

    match &peripheral.device {
        Some(device) => {
            loop {
                info!(
                    "Connecting to Heart Rate Monitor! Name: {:?} | Address: {:?}",
                    peripheral.name, peripheral.address
                );
                let mut battery_checking_interval =
                    tokio::time::interval(Duration::from_secs(60 * 5));
                battery_checking_interval.reset();
                match timeout(duration, device.connect()).await {
                    Ok(Ok(_)) => {
                        if let Some(device) = &peripheral.device {
                            if let Err(e) = device.discover_services().await {
                                error!("Couldn't read services from connected device: {}", e);
                                continue;
                            }
                            let characteristics = device.characteristics();
                            let mut battery_level = BatteryLevel::NotReported;
                            let len = characteristics.len();
                            debug!("Found {} characteristics", len);
                            if let Some(characteristic) = characteristics
                                .iter()
                                .find(|c| c.uuid == BATTERY_LEVEL_CHARACTERISTIC_UUID)
                            {
                                battery_level = device.read(characteristic).await.map_or_else(
                                    |_| {
                                        warn!("Failed to read battery level");
                                        BatteryLevel::Unknown
                                    },
                                    |v| BatteryLevel::Level(v[0]),
                                );
                            }

                            if let Some(characteristic) = characteristics
                                .iter()
                                .find(|c| c.uuid == HEART_RATE_MEASUREMENT_CHARACTERISTIC_UUID)
                            {
                                if device.subscribe(characteristic).await.is_err() {
                                    error!("Failed to subscribe to HR service!");
                                    device.disconnect().await.expect("Failed to disconnect?");
                                    continue;
                                }
                            } else {
                                error!("Didn't find HR service during notification setup!");
                                device.disconnect().await.expect("Failed to disconnect?");
                                continue;
                            }

                            let mut notification_stream = match device.notifications().await {
                                Ok(stream) => stream,
                                Err(e) => {
                                    error!("Failed to get HR BLE notification stream: {}", e);
                                    continue;
                                }
                            };

                            // Assume we have a good connection if we keep getting updates
                            loop {
                                tokio::select! {
                                    // HR update received
                                    Some(data) = notification_stream.next() => {
                                        if data.uuid == HEART_RATE_MEASUREMENT_CHARACTERISTIC_UUID {
                                            let measurement = parse_hrm(&data.value);
                                            let status = HeartRateStatus {
                                                heart_rate_bpm: measurement.bpm,
                                                rr_intervals: measurement.rr_intervals,
                                                battery_level: battery_level,
                                            };
                                            hr_tx.send(DeviceData::HeartRateStatus(status)).expect("Failed to send HR data!");
                                        }
                                    }
                                    // Checking for a new battery level
                                    _ = battery_checking_interval.tick() => {
                                        if let Some(characteristic) = characteristics
                                            .iter()
                                            .find(|c| c.uuid == BATTERY_LEVEL_CHARACTERISTIC_UUID)
                                        {
                                            battery_level = device.read(characteristic).await.map_or_else(
                                                |_| {
                                                    warn!("Failed to refresh battery level, keeping old");
                                                    battery_level
                                                },
                                                |v| BatteryLevel::Level(v[0]),
                                            );
                                        }
                                    }
                                    // Timeout
                                    _ = tokio::time::sleep(duration) => {
                                        error!("No HR data received in {} seconds!", duration.as_secs());
                                        break;
                                    }
                                }
                            }

                            info!("Heart Rate Monitor disconnected (notif thread)!");
                            device.disconnect().await.expect("Failed to disconnect?");
                        }
                    }
                    Ok(Err(e)) => {
                        error!("Connection error: {}", e);
                        hr_tx
                            .send(DeviceData::Error(ErrorPopup::Intermittent(format!(
                                "Connection error: {}",
                                e
                            ))))
                            .expect("Failed to send error message");
                    }
                    Err(_) => {
                        error!("Connection timed out");
                        hr_tx
                            .send(DeviceData::Error(ErrorPopup::Intermittent(
                                "Connection timed out".to_string(),
                            )))
                            .expect("Failed to send error message");
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
        None => {
            error!("Device not found");
            hr_tx
                .send(DeviceData::Error(ErrorPopup::Fatal(
                    "Device not found".to_string(),
                )))
                .expect("Failed to send error message");
        }
    }
}
