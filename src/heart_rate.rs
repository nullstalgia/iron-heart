use crate::app::DeviceData;
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
    pub rr_intervals: Vec<u16>,
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
                match timeout(duration, device.connect()).await {
                    Ok(Ok(_)) => {
                        if let Some(device) = &peripheral.device {
                            if let Err(e) = device.discover_services().await {
                                error!("Couldn't read services from connected device: {}", e);
                                continue;
                            }
                            let characteristics = device.characteristics();
                            let mut on_connect_battery_level = BatteryLevel::NotReported;
                            let len = characteristics.len();
                            debug!("Found {} characteristics", len);
                            if let Some(characteristic) = characteristics
                                .iter()
                                .find(|c| c.uuid == BATTERY_LEVEL_CHARACTERISTIC_UUID)
                            {
                                on_connect_battery_level =
                                    device.read(characteristic).await.map_or_else(
                                        |_| BatteryLevel::Unknown,
                                        |v| BatteryLevel::Level(v[0]),
                                    );
                                if on_connect_battery_level == BatteryLevel::Unknown {
                                    warn!("Failed to read battery level");
                                }
                            }

                            if let Some(characteristic) = characteristics
                                .iter()
                                .find(|c| c.uuid == HEART_RATE_MEASUREMENT_CHARACTERISTIC_UUID)
                            {
                                if device.subscribe(characteristic).await.is_err() {
                                    error!("Failed to subscribe to HR service!");
                                    continue;
                                }
                            } else {
                                error!("Didn't find HR service during notification setup!");
                                continue;
                            }

                            let mut notification_stream = match device.notifications().await {
                                Ok(stream) => stream,
                                Err(e) => {
                                    error!("Failed to get HR BLE notification stream: {}", e);
                                    continue;
                                }
                            };

                            // Process while the BLE connection is not broken or stopped.
                            while let Ok(Some(data)) =
                                timeout(duration, notification_stream.next()).await
                            {
                                if data.uuid == HEART_RATE_MEASUREMENT_CHARACTERISTIC_UUID {
                                    let flags = data.value[0];
                                    let hr_is_u16 = (flags >> 0) & 1;
                                    //let sensor_contacting = (flags >> 1) & 1;
                                    //let sensor_contact_support = (flags >> 2) & 1;
                                    //let energy_expended_support = (flags >> 3) & 1;
                                    let rr_interval_present = (flags >> 4) & 1;

                                    let heart_rate: u16 = if hr_is_u16 == 0 {
                                        data.value[1] as u16
                                    } else {
                                        u16::from_le_bytes([data.value[1], data.value[2]])
                                    };

                                    //status.heart_rate_bpm = heart_rate;

                                    // if rr_interval == 1 {
                                    //     let rr_interval =
                                    //         u16::from_le_bytes([data.value[3], data.value[4]]);
                                    //     status.rr_intervals.push(rr_interval);
                                    // }
                                    let status = HeartRateStatus {
                                        heart_rate_bpm: heart_rate,
                                        rr_intervals: Vec::new(),
                                        battery_level: on_connect_battery_level,
                                    };
                                    let _ = hr_tx.send(DeviceData::HeartRateStatus(status));
                                }
                            }
                            info!("Heart Rate Monitor disconnected (notif thread)!");
                            device.disconnect().await.expect("Failed to disconnect?");
                        }
                    }
                    // TODO Make these semi ephemeral
                    // And also trigger a reconnect
                    Ok(Err(e)) => {
                        let status = HeartRateStatus {
                            heart_rate_bpm: 3,
                            rr_intervals: Vec::new(),
                            battery_level: BatteryLevel::Level(1),
                        };
                        let _ = hr_tx.send(DeviceData::HeartRateStatus(status));
                        hr_tx
                            .send(DeviceData::Error(format!("Connection error: {}", e)))
                            .expect("Failed to send error message");
                    }
                    Err(_) => {
                        let status = HeartRateStatus {
                            heart_rate_bpm: 4,
                            rr_intervals: Vec::new(),
                            battery_level: BatteryLevel::Level(2),
                        };
                        let _ = hr_tx.send(DeviceData::HeartRateStatus(status));
                        hr_tx
                            .send(DeviceData::Error("Connection timed out".to_string()))
                            .expect("Failed to send error message");
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
        None => {
            hr_tx
                .send(DeviceData::Error("Device not found".to_string()))
                .expect("Failed to send error message");
        }
    }
}
