use crate::app::DeviceData;
use crate::structs::{Characteristic, DeviceInfo};
use btleplug::api::{
    Central, CentralEvent, Manager as _, Peripheral, PeripheralProperties, ScanFilter,
};
use btleplug::platform::Manager;
use futures::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use uuid::Uuid;

pub const HEART_RATE_SERVICE_UUID: Uuid = Uuid::from_u128(0x0000180d_0000_1000_8000_00805f9b34fb);

pub const HEART_RATE_MEASUREMENT_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0x00002a37_0000_1000_8000_00805f9b34fb);

pub const BATTERY_LEVEL_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0x00002a19_0000_1000_8000_00805f9b34fb);
pub const BATTERY_SERVICE_UUID: Uuid = Uuid::from_u128(0x0000180f_0000_1000_8000_00805f9b34fb);

#[derive(Debug, Clone, Default)]
pub struct HeartRateStatus {
    pub heart_rate_bpm: u16,
    pub rr_intervals: Vec<u16>,
    pub battery_level: u8,
}

pub enum MonitorData {
    Connected,
    Disconnected,
    HeartRateStatus(HeartRateStatus),
    Error(String),
}
///
pub async fn subscribe_to_heart_rate(
    hr_tx: mpsc::UnboundedSender<MonitorData>,
    peripheral: Arc<DeviceInfo>,
) {
    let duration = Duration::from_secs(10);
    match &peripheral.device {
        Some(device) => match timeout(duration, device.connect()).await {
            Ok(Ok(_)) => {
                if let Some(device) = &peripheral.device {
                    let _ = hr_tx.send(MonitorData::Connected);
                    device.discover_services().await.unwrap();
                    let characteristics = device.characteristics();
                    let mut on_connect_battery_level = 0;

                    for characteristic in characteristics {
                        let uuid = characteristic.uuid;
                        if uuid == HEART_RATE_MEASUREMENT_CHARACTERISTIC_UUID {
                            let _ = device.subscribe(&characteristic).await.unwrap();
                            // TODO Panic if doesn't have????
                        } else if uuid == BATTERY_LEVEL_CHARACTERISTIC_UUID {
                            let value = device.read(&characteristic).await.unwrap();
                            on_connect_battery_level = value[0];
                        }
                    }
                    let mut notification_stream = device.notifications().await.unwrap();
                    // Process while the BLE connection is not broken or stopped.
                    while let Some(data) = notification_stream.next().await {
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
                            let _ = hr_tx.send(MonitorData::HeartRateStatus(status));
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                hr_tx
                    .send(MonitorData::Error(format!("Connection error: {}", e)))
                    .unwrap();
            }
            Err(_) => {
                hr_tx
                    .send(MonitorData::Error("Connection timed out".to_string()))
                    .unwrap();
            }
        },
        None => {
            hr_tx
                .send(MonitorData::Error("Device not found".to_string()))
                .unwrap();
        }
    }
}
