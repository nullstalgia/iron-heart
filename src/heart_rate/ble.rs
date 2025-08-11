use super::{BatteryLevel, HeartRateStatus};
use crate::app::{AppUpdate, ErrorPopup};
use crate::errors::AppError;
use crate::structs::DeviceInfo;

use btleplug::api::{Characteristic, Peripheral, ValueNotification};
use futures::{Stream, StreamExt};
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::broadcast::Sender as BSender;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use crate::broadcast;
use crate::heart_rate::constants::ble_uuids;
use super::measurement::parse_hrm;
use super::miband::{self, MiBandDevice};
use super::twitcher::Twitcher;

struct BleMonitorActor {
    device_info: DeviceInfo,
    rr_cooldown_amount: usize,
    no_packet_timeout: Duration,
    battery_characteristic: Option<Characteristic>,
    cancel_token: CancellationToken,

    battery_level: BatteryLevel,
    twitcher: Twitcher,
    rr_left_to_burn: usize,

    miband_device: Option<MiBandDevice>,
}

// TODO Consider letting this thread be restarted

impl BleMonitorActor {
    async fn connect(
        &mut self,
        broadcast_tx: &BSender<AppUpdate>,
        restart_tx: Sender<()>,
    ) -> Result<(), AppError> {
        'connection: loop {
            let peripheral = self
                .device_info
                .device
                .clone()
                .expect("Missing device object?");
            if self.cancel_token.is_cancelled() {
                break 'connection;
            }
            info!(
                "Connecting to Heart Rate Monitor! Name: {:?} | Address: {:?}",
                self.device_info.name, self.device_info.address
            );
            tokio::select! {
                conn_result = peripheral.connect() => {
                    match conn_result {
                        Ok(_) => {
                            if let Err(e) = peripheral.discover_services().await {
                                error!("Couldn't read services from connected device: {}", e);
                                continue 'connection;
                            }
                            let characteristics = peripheral.characteristics();
                            let len = characteristics.len();
                            debug!("Found {len} characteristics");
                            // Save battery characteristic if present
                            if let Some(characteristic) = characteristics
                                .iter()
                                .find(|c| c.uuid == Uuid::from(ble_uuids::characteristic::BATTERY_LEVEL))
                            {
                                self.battery_characteristic = Some(characteristic.to_owned());
                                self.get_monitor_battery(&peripheral).await;
                            }

                            if let Some(miband_device) = &mut self.miband_device {
                                // Authenticate the MiBand device
                                if let Err(e) = miband_device.authenticate(&peripheral).await {
                                    error!("MiBand authentication failed: {}", e);
                                    //peripheral.disconnect().await?;
                                    broadcast!(broadcast_tx, ErrorPopup::Intermittent(format!(
                                        "MiBand authentication failed: {e}"
                                    )));
                                    continue 'connection;
                                }

                                // Start heart rate monitoring
                                if let Err(e) = miband_device.start_heart_rate_monitor(&peripheral, true).await {
                                    error!("Failed to start heart rate monitoring: {}", e);
                                    //peripheral.disconnect().await?;
                                    broadcast!(broadcast_tx, ErrorPopup::Intermittent(format!(
                                        "Failed to start heart rate monitoring: {e}"
                                    )));
                                    continue 'connection;
                                }

                                info!("MiBand device authenticated and heart rate monitoring started");
                            }

                            // Subscribe to heart rate notifications
                            if let Some(characteristic) = characteristics
                                .iter()
                                .find(|c| c.uuid == Uuid::from(ble_uuids::characteristic::HEART_RATE_MEASUREMENT))
                            {
                                if peripheral.subscribe(characteristic).await.is_err() {
                                    error!("Failed to subscribe to HR service!");
                                    peripheral.disconnect().await?;
                                    continue 'connection;
                                }
                            } else {
                                error!("Didn't find HR service during notification setup!");
                                peripheral.disconnect().await?;
                                continue 'connection;
                            }

                            let notification_stream = match peripheral.notifications().await {
                                Ok(stream) => stream,
                                Err(e) => {
                                    error!("Failed to get HR BLE notification stream: {}", e);
                                    peripheral.disconnect().await?;
                                    continue 'connection;
                                }
                            };

                            self.notification_loop(broadcast_tx, notification_stream, &peripheral).await?;

                            info!("Heart Rate Monitor stream closed!");
                            peripheral.disconnect().await?;
                            if self.cancel_token.is_cancelled() {
                                break 'connection;
                            }
                            broadcast!(broadcast_tx, ErrorPopup::Intermittent(
                                "Connection timed out".into(),
                            ));
                        }
                        Err(e) => {
                            peripheral.disconnect().await?;

                            error!("BLE Connection error: {}", e);
                            broadcast!(broadcast_tx, ErrorPopup::Intermittent(format!(
                                "BLE Connection error: {e}"

                            )));
                            // `NotConnected` is the "Device Unreachable" error
                            // Weirdly enough, the Central manager doesn't get that error, only we do here at the HR level
                            // `DeviceNotFound` can also occur when being disconnected for a long time
                            // Telling the manager to restart its scan when these crop up help avoid needing to restart the whole app
                            match e {
                                btleplug::Error::NotConnected | btleplug::Error::DeviceNotFound => {
                                    restart_tx.send(()).await.expect("Couldn't trigger BLE Scan!");
                                },
                                _ => {}
                            }
                            tokio::time::sleep(Duration::from_secs(20)).await;
                        }
                    }
                }
                _ = self.cancel_token.cancelled() => {
                    if peripheral.is_connected().await.unwrap_or(false) {
                        peripheral.disconnect().await?;
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
            tokio::time::sleep(Duration::from_secs(10)).await;
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
        let mut last_packet = std::time::Instant::now();

        loop {
            tokio::select! {
                // Assume we have a good connection if we keep getting updates
                // HR update received
                Some(data) = notification_stream.next() => {
                    last_packet = std::time::Instant::now();

                    if data.uuid == Uuid::from(ble_uuids::characteristic::HEART_RATE_MEASUREMENT) {
                        let hr = self.handle_ble_hr(&data);
                        broadcast!(broadcast_tx, hr);
                    }
                }
                _ = battery_checking_interval.tick() => {
                    self.get_monitor_battery(device).await;
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    if last_packet.elapsed() > self.no_packet_timeout {
                        error!("No packets received for {:?}s, disconnecting", self.no_packet_timeout.as_secs());
                        return Ok(());
                    }
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
    device_info: DeviceInfo,
    rr_cooldown_amount: usize,
    twitch_threshold: f32,
    no_packet_timeout: Duration,
    cancel_token: CancellationToken,
) {
    let battery_level = BatteryLevel::NotReported;

    // Check if the device is a MiBand
    let miband_device: Option<MiBandDevice> = if miband::is_miband(&device_info) {
        Option::from(MiBandDevice::new(device_info.clone()))
    } else {
        None
    };

    let mut ble_monitor = BleMonitorActor {
        device_info,
        no_packet_timeout,
        battery_characteristic: None,
        cancel_token,
        battery_level,
        twitcher: Twitcher::new(twitch_threshold),
        rr_cooldown_amount,
        rr_left_to_burn: rr_cooldown_amount,
        miband_device,
    };

    if let Err(e) = ble_monitor.connect(&broadcast_tx, restart_tx).await {
        error!("Fatal BLE Error: {e}");
        let message = "Fatal BLE Error";
        broadcast!(broadcast_tx, ErrorPopup::detailed(message, e));
    }
}
