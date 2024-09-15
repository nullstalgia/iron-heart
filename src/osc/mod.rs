use addresses::OscAddresses;
use hr::{make_mimic_data, send_raw_beat_params, send_raw_hr_status};
use log::*;
use rosc::OscTime;
use std::net::{SocketAddrV4, UdpSocket};
use std::str::FromStr;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::{Receiver as BReceiver, Sender as BSender};
use tokio::time::{self, interval, Duration, Instant, Interval};
use tokio_util::sync::CancellationToken;

use crate::app::{AppUpdate, ErrorPopup};
use crate::broadcast;
use crate::errors::AppError;
use crate::heart_rate::{rr_from_bpm, HeartRateStatus};
use crate::settings::OscSettings;

mod addresses;
mod hr;

const OSC_NOW: OscTime = OscTime {
    seconds: 0,
    fractional: 0,
};

struct OscActor {
    // I/O and current data
    target_addr: SocketAddrV4,
    hr_status: HeartRateStatus,
    //
    osc_settings: OscSettings,
    socket: UdpSocket,
    osc_addresses: OscAddresses,
    // Used to delay the connected bool by one update "cycle",
    // as otherwise a value of "0" can sneak in on the display.
    delay_sending_connected: bool,
    //
    positive_float_bpm: bool,
    use_real_rr: bool,
    latest_rr: Duration,
    // This interval flip/flops between the RR duration and the pulse duration
    // to allow sending a pulse without a blocking sleep() call
    heart_beat_ticker: Interval,
    beat_pulse: Duration,
    pulse_edge: bool,
    toggle_edge: bool,
    disconnected_at: Option<Instant>,
    disconnect_update_interval: Interval,
    // Used when BLE connection is lost, but we don't want to
    // hide the BPM display in VRChat, we'll just bounce around
    // the last known actual value until we reconnect or time out.
    max_hide_disconnection: Duration,
}

impl OscActor {
    fn build(osc_settings: OscSettings) -> Result<Self, AppError> {
        let osc_addresses = OscAddresses::build(&osc_settings)?;

        let host_addr = SocketAddrV4::from_str(&format!("{}:{}", osc_settings.host_ip, 0))?;

        let target_addr =
            SocketAddrV4::from_str(&format!("{}:{}", osc_settings.target_ip, osc_settings.port))?;

        let socket = UdpSocket::bind(host_addr)?;

        let beat_pulse_duration = Duration::from_millis(osc_settings.pulse_length_ms as u64);
        let positive_float_bpm = osc_settings.only_positive_float_bpm;

        let disconnect_update_interval = time::interval(Duration::from_secs(6));

        let max_hide_disconnection =
            Duration::from_secs(osc_settings.max_hide_disconnection_sec as u64);

        Ok(OscActor {
            target_addr,
            delay_sending_connected: true,
            positive_float_bpm,
            use_real_rr: false,
            latest_rr: Duration::from_secs(1),
            socket,
            osc_settings,
            osc_addresses,
            hr_status: HeartRateStatus::default(),
            heart_beat_ticker: interval(Duration::from_secs(1)),
            beat_pulse: beat_pulse_duration,
            pulse_edge: false,
            toggle_edge: false,
            disconnected_at: None,
            disconnect_update_interval,
            max_hide_disconnection,
        })
    }
    // Hides display on avatar and sets value to 0
    // Used on startup, disconnect, and shutdown
    fn init_params(&mut self) -> Result<(), AppError> {
        self.delay_sending_connected = true;
        self.toggle_edge = false;
        send_raw_hr_status(
            &HeartRateStatus::default(),
            false,
            false,
            self.positive_float_bpm,
            &self.osc_addresses,
            &self.socket,
            self.target_addr,
        )?;
        send_raw_beat_params(
            false,
            false,
            &self.osc_addresses,
            &self.socket,
            self.target_addr,
        )?;
        Ok(())
    }
    fn handle_data(&mut self, data: HeartRateStatus) -> Result<(), AppError> {
        // Fresh BPM data!
        if data.heart_rate_bpm > 0 {
            self.hr_status = data;
            self.disconnected_at = None;
            if let Some(new_rr) = self.hr_status.rr_intervals.last() {
                self.latest_rr = *new_rr;
                // Mark that we know we'll get real RR intervals
                // (don't need to calculate from BPM from now on)
                self.use_real_rr = true;
            } else if !self.use_real_rr {
                self.latest_rr = rr_from_bpm(self.hr_status.heart_rate_bpm);
            }
        // Got a 0 BPM packet
        // This can be due to either a disconnection,
        // *or* the actual Monitor itself initializing and sending 0.
        // Since showing 0 BPM on the avatar isn't ideal,
        // those cases are treated equally.
        } else if self.osc_settings.hide_disconnections {
            self.disconnected_at.get_or_insert(Instant::now());
        } else {
            self.hr_status = data;
            self.init_params()?;
            return Ok(());
        }

        // Param that goes true when we're sending mimic data
        let hiding_ble_disconnection = if let Some(dc_timestamp) = self.disconnected_at {
            (dc_timestamp.elapsed() < self.max_hide_disconnection)
                && (self.hr_status.heart_rate_bpm > 0)
        } else {
            false
        };

        send_raw_hr_status(
            &self.hr_status,
            hiding_ble_disconnection,
            self.delay_sending_connected,
            self.positive_float_bpm,
            &self.osc_addresses,
            &self.socket,
            self.target_addr,
        )?;
        // Check after sending, otherwise it's pointless
        if self.delay_sending_connected && (self.hr_status.heart_rate_bpm > 0) {
            self.delay_sending_connected = false;
        }
        Ok(())
    }
    // Ran by the `heart_beat_ticker`'s tick()
    // And modifies the interval on each tick
    // to send short pulses without blocking
    fn heart_beat(&mut self) -> Result<(), AppError> {
        if self.hr_status.heart_rate_bpm > 0 && !self.delay_sending_connected {
            if !self.pulse_edge {
                // Rising edge
                self.pulse_edge = true;
                self.toggle_edge = !self.toggle_edge;
                self.heart_beat_ticker = time::interval(self.beat_pulse);
                self.heart_beat_ticker.reset();
            } else {
                // Falling edge
                self.pulse_edge = false;
                let new_interval = self.latest_rr.saturating_sub(self.beat_pulse);
                self.heart_beat_ticker = time::interval(new_interval);
                self.heart_beat_ticker.reset();
            }
            send_raw_beat_params(
                self.pulse_edge,
                self.toggle_edge,
                &self.osc_addresses,
                &self.socket,
                self.target_addr,
            )?;
        }
        Ok(())
    }
    fn mimic_tick(&mut self) -> Result<(), AppError> {
        if let Some(dc_timestamp) = self.disconnected_at {
            let hiding_ble_disconnection = (dc_timestamp.elapsed() < self.max_hide_disconnection)
                && (self.hr_status.heart_rate_bpm > 0);

            if hiding_ble_disconnection {
                let mimic = make_mimic_data(&self.hr_status);
                send_raw_hr_status(
                    &mimic,
                    hiding_ble_disconnection,
                    self.delay_sending_connected,
                    self.positive_float_bpm,
                    &self.osc_addresses,
                    &self.socket,
                    self.target_addr,
                )?;
            } else {
                // Alright, we're really disconnected now
                self.hr_status = HeartRateStatus::default();
                self.init_params()?;
            }
        }
        Ok(())
    }
    async fn rx_loop(
        &mut self,
        mut broadcast_rx: BReceiver<AppUpdate>,
        cancel_token: CancellationToken,
    ) -> Result<(), AppError> {
        self.init_params()?;

        loop {
            let heart_beat = self.heart_beat_ticker.tick();
            let mimic = self.disconnect_update_interval.tick();
            tokio::select! {
                hr_data = broadcast_rx.recv() => {
                    match hr_data {
                        Ok(AppUpdate::HeartRateStatus(data)) => {
                            self.handle_data(data)?;
                        },
                        Ok(_) => {},
                        Err(RecvError::Closed) => {
                            error!("OSC: Channel closed");
                            self.init_params()?;
                            break;
                        },
                        Err(RecvError::Lagged(count)) => {
                            warn!("OSC: Lagged! Missed {count} messages");
                        }
                    }
                }
                // Sending params for each heart beat, based on the measured interval
                _ = heart_beat => {
                    self.heart_beat()?;
                }
                // Sending mimic data when we're disconnected
                _ = mimic => {
                    self.mimic_tick()?;
                }
                _ = cancel_token.cancelled() => {
                    info!("Shutting down OSC thread!");
                    self.init_params()?;
                    break;
                }
            }
        }
        Ok(())
    }
}

pub async fn osc_thread(
    broadcast_rx: BReceiver<AppUpdate>,
    broadcast_tx: BSender<AppUpdate>,
    osc_settings: OscSettings,
    cancel_token: CancellationToken,
) {
    let mut osc = match OscActor::build(osc_settings) {
        Ok(osc) => osc,
        Err(e) => {
            error!("Failed to set up OSC. {e}");
            let message = format!("Failed to set up OSC. {e}");
            broadcast!(broadcast_tx, ErrorPopup::Fatal(message));
            return;
        }
    };

    // TODO?
    // Maybe option for twitches to be a toggle and/or pulse?
    // Current implementation is a weird mix of both, but is simple to implement

    if let Err(e) = osc.rx_loop(broadcast_rx, cancel_token).await {
        error!("OSC Error: {e}");
        let message = format!("OSC Error: {e}");
        broadcast!(broadcast_tx, ErrorPopup::Fatal(message));
    }
}
