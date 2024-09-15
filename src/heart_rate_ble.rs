use crate::app::{AppUpdate, DeviceUpdate, ErrorPopup};
use crate::errors::AppError;
use crate::heart_rate::{BatteryLevel, HeartRateStatus};
use crate::heart_rate_measurement::parse_hrm;
use crate::structs::DeviceInfo;

use btleplug::api::{Characteristic, Peripheral, ValueNotification};
use futures::{Stream, StreamExt};
use log::*;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::{Receiver as BReceiver, Sender as BSender};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const HEART_RATE_SERVICE_UUID: Uuid = Uuid::from_u128(0x0000180d_0000_1000_8000_00805f9b34fb);
pub const HEART_RATE_MEASUREMENT_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0x00002a37_0000_1000_8000_00805f9b34fb);

//pub const BATTERY_SERVICE_UUID: Uuid = Uuid::from_u128(0x0000180f_0000_1000_8000_00805f9b34fb);
pub const BATTERY_LEVEL_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0x00002a19_0000_1000_8000_00805f9b34fb);

struct BleMonitorActor {
    peripheral: DeviceInfo,
    rr_cooldown_amount: usize,
    twitch_threshold: f32,
    no_packet_timeout: Duration,
    battery_characteristic: Option<Characteristic>,
    cancel_token: CancellationToken,

    battery_level: BatteryLevel,
    latest_rr: Duration,
    rr_left_to_burn: usize,
}

impl BleMonitorActor {
    async fn connect(&mut self, hr_tx: &BSender<AppUpdate>) -> Result<(), AppError> {
        let device = self
            .peripheral
            .device
            .clone()
            .expect("Missing device object?");
        'connection: loop {
            if self.cancel_token.is_cancelled() {
                break 'connection;
            }
            info!(
                "Connecting to Heart Rate Monitor! Name: {:?} | Address: {:?}",
                self.peripheral.name, self.peripheral.address
            );
            tokio::select! {
                conn_result = device.connect() => {
                    match conn_result {
                        Ok(_) => {
                            if let Err(e) = device.discover_services().await {
                                error!("Couldn't read services from connected device: {}", e);
                                continue 'connection;
                            }
                            let characteristics = device.characteristics();
                            let len = characteristics.len();
                            debug!("Found {len} characteristics");
                            // Save battery characteristic if present
                            if let Some(characteristic) = characteristics
                                .iter()
                                .find(|c| c.uuid == BATTERY_LEVEL_CHARACTERISTIC_UUID)
                            {
                                self.battery_characteristic = Some(characteristic.to_owned());
                                self.get_monitor_battery(&device).await;
                            }

                            if let Some(characteristic) = characteristics
                                .iter()
                                .find(|c| c.uuid == HEART_RATE_MEASUREMENT_CHARACTERISTIC_UUID)
                            {
                                if device.subscribe(characteristic).await.is_err() {
                                    error!("Failed to subscribe to HR service!");
                                    device.disconnect().await?;
                                    continue 'connection;
                                }
                            } else {
                                error!("Didn't find HR service during notification setup!");
                                device.disconnect().await?;
                                continue 'connection;
                            }

                            let notification_stream = match device.notifications().await {
                                Ok(stream) => stream,
                                Err(e) => {
                                    error!("Failed to get HR BLE notification stream: {}", e);
                                    device.disconnect().await?;
                                    continue 'connection;
                                }
                            };

                            self.notification_loop(hr_tx, notification_stream, &device).await?;

                            info!("Heart Rate Monitor stream closed!");
                            device.disconnect().await?;
                            hr_tx
                                .send(AppUpdate::Error(ErrorPopup::Intermittent(
                                    "Connection timed out".to_string(),
                                )))
                                .expect("Failed to send error message");
                        }
                        Err(e) => {
                            error!("BLE Connection error: {}", e);
                            hr_tx
                                .send(AppUpdate::Error(ErrorPopup::Intermittent(format!(
                                    "BLE Connection error: {}",
                                    e
                                ))))
                                .expect("Failed to send error message");
                        }
                    }
                }
                _ = self.cancel_token.cancelled() => {
                    if device.is_connected().await.unwrap_or(false) {
                        device.disconnect().await?;
                    }
                    break 'connection;
                }
                _ = tokio::time::sleep(self.no_packet_timeout) => {
                    error!("Connection timed out");
                    hr_tx
                        .send(AppUpdate::Error(ErrorPopup::Intermittent(
                            "Connection timed out".to_string(),
                        )))
                        .expect("Failed to send error message");
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Ok(())
    }
    async fn notification_loop(
        &mut self,
        hr_tx: &BSender<AppUpdate>,
        mut notification_stream: Pin<Box<dyn Stream<Item = ValueNotification> + Send>>,
        device: &btleplug::platform::Peripheral,
    ) -> Result<(), AppError> {
        let mut battery_checking_interval = tokio::time::interval(Duration::from_secs(60 * 5));
        loop {
            tokio::select! {
                // Assume we have a good connection if we keep getting updates
                // HR update received
                Some(data) = notification_stream.next() => {
                    if data.uuid == HEART_RATE_MEASUREMENT_CHARACTERISTIC_UUID {
                        let hr = self.handle_ble_hr(&data);
                        hr_tx.send(AppUpdate::HeartRateStatus(hr)).expect("Failed to send HR data!");
                    }
                }
                _ = battery_checking_interval.tick() => {
                    self.get_monitor_battery(device).await;
                }
                _ = tokio::time::sleep(self.no_packet_timeout) => {
                    error!("No HR data received in {} seconds!", self.no_packet_timeout.as_secs());
                    return Ok(());
                }
                _ = self.cancel_token.cancelled() => {
                    info!("Shutting down HR Notification thread!");
                    return Ok(());
                }
            }
        }
    }
    fn handle_ble_hr(&mut self, data: &ValueNotification) -> HeartRateStatus {
        let new_hr_status = parse_hrm(&data.value);
        // An oddity I've noticed, is if we don't get an RR interval each update,
        // there's a decent chance that the next one we do get will be weirdly high.
        // So we'll just ignore the first few values we get after an empty set.
        let new_interval_count = new_hr_status.rr_intervals.len();
        let rr_intervals = if new_interval_count > self.rr_left_to_burn {
            new_hr_status.rr_intervals[self.rr_left_to_burn..].to_vec()
        } else {
            Vec::new()
        };
        self.rr_left_to_burn = if self.rr_left_to_burn == 0 && new_hr_status.rr_intervals.is_empty()
        {
            self.rr_cooldown_amount
        } else {
            self.rr_left_to_burn.saturating_sub(new_interval_count)
        };
        let mut twitch_up = false;
        let mut twitch_down = false;
        for new_rr in rr_intervals.iter() {
            // Duration.abs_diff() is nightly only for now, agh
            if (new_rr.as_secs_f32() - self.latest_rr.as_secs_f32()).abs() > self.twitch_threshold {
                if *new_rr > self.latest_rr {
                    twitch_up = true;
                } else {
                    twitch_down = true;
                }
            }
            self.latest_rr = *new_rr;
        }
        HeartRateStatus {
            heart_rate_bpm: new_hr_status.bpm,
            rr_intervals,
            battery_level: self.battery_level,
            twitch_up,
            twitch_down,
        }
    }
    async fn get_monitor_battery(&mut self, device: &btleplug::platform::Peripheral) {
        if let Some(characteristic) = self.battery_characteristic.as_ref() {
            self.battery_level = device.read(characteristic).await.map_or_else(
                |_| {
                    warn!("Failed to refresh battery level, keeping last");
                    self.battery_level
                },
                |v| BatteryLevel::Level(v[0]),
            );
        }
    }
}

pub async fn start_notification_thread(
    hr_tx: BSender<AppUpdate>,
    peripheral: DeviceInfo,
    rr_cooldown_amount: usize,
    twitch_threshold: f32,
    cancel_token: CancellationToken,
) {
    let no_packet_timeout = Duration::from_secs(30);
    let battery_level = BatteryLevel::NotReported;
    let mut ble_monitor = BleMonitorActor {
        peripheral,
        twitch_threshold,
        no_packet_timeout,
        battery_characteristic: None,
        cancel_token,
        battery_level,
        latest_rr: Duration::from_secs(1),
        rr_cooldown_amount,
        rr_left_to_burn: rr_cooldown_amount,
    };

    if let Err(e) = ble_monitor.connect(&hr_tx).await {
        error!("Fatal BLE Error: {e}");
        let message = format!("Fatal BLE Error: {e}");
        hr_tx.send(AppUpdate::Error(ErrorPopup::Fatal(message)));
    }
}
