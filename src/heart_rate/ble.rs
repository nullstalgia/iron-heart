use super::{BatteryLevel, HeartRateStatus};
use crate::app::{AppUpdate, ErrorPopup};
use crate::errors::AppError;
use crate::structs::DeviceInfo;

use btleplug::api::{Characteristic, Peripheral, ValueNotification};
use futures::{Stream, StreamExt};
use log::*;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::broadcast::Sender as BSender;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::broadcast;

use super::measurement::parse_hrm;
use super::twitcher::Twitcher;

pub const HEART_RATE_SERVICE_UUID: Uuid = Uuid::from_u128(0x0000180d_0000_1000_8000_00805f9b34fb);
pub const HEART_RATE_MEASUREMENT_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0x00002a37_0000_1000_8000_00805f9b34fb);

//pub const BATTERY_SERVICE_UUID: Uuid = Uuid::from_u128(0x0000180f_0000_1000_8000_00805f9b34fb);
pub const BATTERY_LEVEL_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0x00002a19_0000_1000_8000_00805f9b34fb);

struct BleMonitorActor {
    peripheral: DeviceInfo,
    rr_cooldown_amount: usize,
    no_packet_timeout: Duration,
    battery_characteristic: Option<Characteristic>,
    cancel_token: CancellationToken,

    battery_level: BatteryLevel,
    twitcher: Twitcher,
    rr_left_to_burn: usize,
}

impl BleMonitorActor {
    async fn connect(
        &mut self,
        broadcast_tx: &BSender<AppUpdate>,
        restart_tx: Sender<()>,
    ) -> Result<(), AppError> {
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

                            self.notification_loop(broadcast_tx, notification_stream, &device).await?;

                            info!("Heart Rate Monitor stream closed!");
                            device.disconnect().await?;
                            broadcast!(broadcast_tx, ErrorPopup::Intermittent(
                                "Connection timed out".into(),
                            ));
                        }
                        Err(e) => {
                            error!("BLE Connection error: {}", e);
                            broadcast!(broadcast_tx, ErrorPopup::Intermittent(format!(
                                "BLE Connection error: {}",
                                e
                            )));
                            // This is the "Device Unreachable" error
                            // Weirdly enough, the Central manager doesn't get this error, only we do here at the HR level
                            // So, we'll just restart the BLE manager to try to avoid continuous failed reconnects
                            if let btleplug::Error::NotConnected = e {
                                restart_tx.send(()).await.expect("Couldn't restart BLE Manager!");
                                tokio::time::sleep(Duration::from_secs(3)).await;
                            }
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
                    broadcast!(broadcast_tx, ErrorPopup::Intermittent(
                        "Connection timed out".into(),
                    ));
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Ok(())
    }
    async fn notification_loop(
        &mut self,
        broadcast_tx: &BSender<AppUpdate>,
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
                        broadcast!(broadcast_tx, hr);
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
        let timestamp = chrono::Local::now();
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
        let (twitch_up, twitch_down) = self.twitcher.handle(new_hr_status.bpm, &rr_intervals);

        HeartRateStatus {
            heart_rate_bpm: new_hr_status.bpm,
            rr_intervals,
            battery_level: self.battery_level,
            twitch_up,
            twitch_down,
            timestamp,
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
    broadcast_tx: BSender<AppUpdate>,
    restart_tx: Sender<()>,
    peripheral: DeviceInfo,
    rr_cooldown_amount: usize,
    twitch_threshold: f32,
    cancel_token: CancellationToken,
) {
    let no_packet_timeout = Duration::from_secs(30);
    let battery_level = BatteryLevel::NotReported;
    let mut ble_monitor = BleMonitorActor {
        peripheral,
        no_packet_timeout,
        battery_characteristic: None,
        cancel_token,
        battery_level,
        twitcher: Twitcher::new(twitch_threshold),
        rr_cooldown_amount,
        rr_left_to_burn: rr_cooldown_amount,
    };

    if let Err(e) = ble_monitor.connect(&broadcast_tx, restart_tx).await {
        error!("Fatal BLE Error: {e}");
        let message = "Fatal BLE Error";
        broadcast!(broadcast_tx, ErrorPopup::detailed(message, e));
    }
}
