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
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const HEART_RATE_SERVICE_UUID: Uuid = Uuid::from_u128(0x0000180d_0000_1000_8000_00805f9b34fb);

pub const HEART_RATE_MEASUREMENT_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0x00002a37_0000_1000_8000_00805f9b34fb);

pub const BATTERY_LEVEL_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0x00002a19_0000_1000_8000_00805f9b34fb);
pub const BATTERY_SERVICE_UUID: Uuid = Uuid::from_u128(0x0000180f_0000_1000_8000_00805f9b34fb);

const RR_COOLDOWN_AMOUNT: usize = 3;

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
    pub rr_intervals: Vec<Duration>,
    pub battery_level: BatteryLevel,
    // Twitches are calculated in this file so that
    // all listeners see twitches at the same time
    pub twitch_up: bool,
    pub twitch_down: bool,
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
    twitch_threshold: f32,
    shutdown_token: CancellationToken,
) {
    let duration = Duration::from_secs(30);

    match &peripheral.device {
        Some(device) => {
            'connection: loop {
                info!(
                    "Connecting to Heart Rate Monitor! Name: {:?} | Address: {:?}",
                    peripheral.name, peripheral.address
                );
                if shutdown_token.is_cancelled() {
                    break 'connection;
                }
                let mut battery_checking_interval =
                    tokio::time::interval(Duration::from_secs(60 * 5));
                battery_checking_interval.reset();
                tokio::select! {
                    conn_result = device.connect() => {
                        match conn_result {
                            Ok(_) => {
                                if let Some(device) = &peripheral.device {
                                    if let Err(e) = device.discover_services().await {
                                        error!("Couldn't read services from connected device: {}", e);
                                        continue 'connection;
                                    }
                                    let characteristics = device.characteristics();
                                    let mut battery_level = BatteryLevel::NotReported;
                                    let mut latest_rr: Duration = Duration::from_secs(1);
                                    let mut rr_cooldown = RR_COOLDOWN_AMOUNT;
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
                                            continue 'connection;
                                        }
                                    } else {
                                        error!("Didn't find HR service during notification setup!");
                                        device.disconnect().await.expect("Failed to disconnect?");
                                        continue 'connection;
                                    }

                                    let mut notification_stream = match device.notifications().await {
                                        Ok(stream) => stream,
                                        Err(e) => {
                                            error!("Failed to get HR BLE notification stream: {}", e);
                                            continue 'connection;
                                        }
                                    };

                                    // Assume we have a good connection if we keep getting updates
                                    'updates: loop {
                                        tokio::select! {
                                            // HR update received
                                            Some(data) = notification_stream.next() => {
                                                if data.uuid == HEART_RATE_MEASUREMENT_CHARACTERISTIC_UUID {
                                                    let measurement = parse_hrm(&data.value);
                                                    // An oddity I've noticed, is if we don't get an RR interval each update,
                                                    // there's a decent chance that the next one we do get will be weirdly high.
                                                    // So we'll just ignore the first few values we get after an empty set.
                                                    let new_interval_count = measurement.rr_intervals.len();
                                                    let rr_intervals = if new_interval_count > rr_cooldown {
                                                        measurement.rr_intervals[rr_cooldown..].to_vec()
                                                    } else {
                                                        Vec::new()
                                                    };
                                                    rr_cooldown = if rr_cooldown == 0 && measurement.rr_intervals.is_empty() {
                                                        RR_COOLDOWN_AMOUNT
                                                    } else {
                                                        rr_cooldown.saturating_sub(new_interval_count)
                                                    };
                                                    let mut twitch_up = false;
                                                    let mut twitch_down = false;
                                                    for new_rr in rr_intervals.iter() {
                                                        // Duration.abs_diff() is nightly only for now, agh
                                                        if (new_rr.as_secs_f32() - latest_rr.as_secs_f32()).abs() > twitch_threshold {
                                                            if new_rr > &latest_rr {
                                                                twitch_up = true;
                                                            } else {
                                                                twitch_down = true;
                                                            }
                                                        }
                                                        latest_rr = *new_rr;
                                                    }
                                                    let status = HeartRateStatus {
                                                        heart_rate_bpm: measurement.bpm,
                                                        rr_intervals,
                                                        battery_level,
                                                        twitch_up,
                                                        twitch_down,
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
                                            _ = shutdown_token.cancelled() => {
                                                info!("Shutting down HR Notification thread!");
                                                if device.is_connected().await.unwrap_or(false) {
                                                    device.disconnect().await.expect("Failed to disconnect?");
                                                }
                                                break 'connection;
                                            }
                                            // Timeout
                                            _ = tokio::time::sleep(duration) => {
                                                error!("No HR data received in {} seconds!", duration.as_secs());
                                                break 'updates;
                                            }
                                        }
                                    }
                                    info!("Heart Rate Monitor disconnected (notif thread)!");
                                    device.disconnect().await.expect("Failed to disconnect?");
                                    hr_tx
                                        .send(DeviceData::Error(ErrorPopup::Intermittent(
                                            "Connection timed out".to_string(),
                                        )))
                                        .expect("Failed to send error message");
                                }
                            }
                            Err(e) => {
                                error!("Connection error: {}", e);
                                hr_tx
                                    .send(DeviceData::Error(ErrorPopup::Intermittent(format!(
                                        "Connection error: {}",
                                        e
                                    ))))
                                    .expect("Failed to send error message");
                            }
                        }
                    }
                    _ = shutdown_token.cancelled() => {
                        break 'connection;
                    }
                    _ = tokio::time::sleep(duration) => {
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
