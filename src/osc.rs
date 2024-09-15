use log::*;
use rand::Rng;
use ratatui::widgets::Dataset;
use rosc::address::verify_address;
use rosc::{encoder, OscError};
use rosc::{OscBundle, OscMessage, OscPacket, OscTime, OscType};
use std::f32;
use std::net::{SocketAddrV4, UdpSocket};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::{Receiver as BReceiver, Sender as BSender};
use tokio::sync::mpsc::Sender;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{self, interval, Duration, Instant, Interval};
use tokio_util::sync::CancellationToken;

use crate::app::{AppUpdate, DeviceUpdate, ErrorPopup};
use crate::errors::AppError;
use crate::heart_rate::{rr_from_bpm, BatteryLevel, HeartRateStatus};
use crate::settings::OscSettings;

const OSC_NOW: OscTime = OscTime {
    seconds: 0,
    fractional: 0,
};

struct OscActor {
    // I/O and current data
    target_addr: SocketAddrV4,
    host_addr: SocketAddrV4,
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
            host_addr,
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
        );
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
        mut osc_rx: BReceiver<AppUpdate>,
        cancel_token: CancellationToken,
    ) -> Result<(), AppError> {
        self.init_params()?;

        loop {
            let heart_beat = self.heart_beat_ticker.tick();
            let mimic = self.disconnect_update_interval.tick();
            tokio::select! {
                hr_data = osc_rx.recv() => {
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
    osc_rx: BReceiver<AppUpdate>,
    osc_tx: BSender<AppUpdate>,
    osc_settings: OscSettings,
    cancel_token: CancellationToken,
) {
    let mut osc = match OscActor::build(osc_settings) {
        Ok(osc) => osc,
        Err(e) => {
            error!("Failed to set up OSC. {e}");
            let message = format!("Failed to set up OSC. {e}");
            osc_tx
                .send(AppUpdate::Error(ErrorPopup::Fatal(message)))
                .expect("Failed to send error message");
            return;
        }
    };

    // TODO?
    // Maybe option for twitches to be a toggle and/or pulse?
    // Current implementation is a weird mix of both, but is simple to implement

    if let Err(e) = osc.rx_loop(osc_rx, cancel_token).await {
        error!("OSC Error: {e}");
        let message = format!("OSC Error: {e}");
        osc_tx
            .send(AppUpdate::Error(ErrorPopup::Fatal(message)))
            .expect("Failed to send error message");
    }
}

fn send_raw_hr_status(
    hr_status: &HeartRateStatus,
    hiding_disconnect: bool,
    delay_sending_connected: bool,
    positive_float_bpm: bool,
    osc_addresses: &OscAddresses,
    socket: &UdpSocket,
    target_addr: SocketAddrV4,
) -> Result<(), AppError> {
    let bundle = form_bpm_bundle(
        hr_status,
        hiding_disconnect,
        delay_sending_connected,
        positive_float_bpm,
        osc_addresses,
    );
    let msg_buf = encoder::encode(&OscPacket::Bundle(bundle))?;
    socket.send_to(&msg_buf, target_addr)?;
    Ok(())
}

fn send_raw_beat_params(
    pulse_edge: bool,
    toggle_beat: bool,
    osc_addresses: &OscAddresses,
    socket: &UdpSocket,
    target_addr: SocketAddrV4,
) -> Result<(), AppError> {
    let mut bundle = OscBundle {
        timetag: OSC_NOW,
        content: vec![],
    };

    let pulse_msg = OscMessage {
        addr: osc_addresses.beat_pulse.clone(),
        args: vec![OscType::Bool(pulse_edge)],
    };

    let toggle_msg = OscMessage {
        addr: osc_addresses.beat_toggle.clone(),
        args: vec![OscType::Bool(toggle_beat)],
    };

    bundle.content.push(OscPacket::Message(pulse_msg));
    bundle.content.push(OscPacket::Message(toggle_msg));

    let msg_buf = encoder::encode(&OscPacket::Bundle(bundle))?;
    socket.send_to(&msg_buf, target_addr)?;
    Ok(())
}

struct OscAddresses {
    beat_toggle: String,
    beat_pulse: String,
    bpm_int: String,
    bpm_float: String,
    connected: String,
    hiding_disconnect: String,
    latest_rr: String,
    battery_int: String,
    battery_float: String,
    rr_twitch_up: String,
    rr_twitch_down: String,
}

// Not sure if rosc has a function for this already
fn remove_double_slashes(address: &mut String) {
    while let Some(pos) = address.find("//") {
        address.replace_range(pos..pos + 2, "/");
    }
}

fn remove_trailing_char(s: &mut String, ch: char) {
    if s.ends_with(ch) {
        s.pop();
    }
}

fn format_prefix(prefix: &str) -> Result<String, OscError> {
    let mut address = String::from("/");
    address.push_str(prefix);
    remove_double_slashes(&mut address);
    remove_trailing_char(&mut address, '/');
    if verify_address(&address).is_ok() {
        Ok(address)
    } else {
        Err(OscError::BadAddress(format!(
            "Invalid OSC Prefix: \"{prefix}\""
        )))
    }
}

fn format_address(prefix: &str, param: &str, param_name: &str) -> Result<String, OscError> {
    let mut address = format!("{}/{}", prefix, param);
    remove_double_slashes(&mut address);
    remove_trailing_char(&mut address, '/');
    if verify_address(&address).is_ok() {
        Ok(address)
    } else {
        Err(OscError::BadAddress(format!(
            "Invalid OSC Address: \"{param_name}\": \"{param}\""
        )))
    }
}

impl OscAddresses {
    fn build(osc_settings: &OscSettings) -> Result<Self, OscError> {
        let prefix = format_prefix(&osc_settings.address_prefix)?;
        Ok(OscAddresses {
            beat_toggle: format_address(
                &prefix,
                &osc_settings.param_beat_toggle,
                "param_beat_toggle",
            )?,
            beat_pulse: format_address(
                &prefix,
                &osc_settings.param_beat_pulse,
                "param_beat_pulse",
            )?,
            bpm_int: format_address(&prefix, &osc_settings.param_bpm_int, "param_bpm_int")?,
            bpm_float: format_address(&prefix, &osc_settings.param_bpm_float, "param_bpm_float")?,
            connected: format_address(
                &prefix,
                &osc_settings.param_hrm_connected,
                "param_hrm_connected",
            )?,
            hiding_disconnect: format_address(
                &prefix,
                &osc_settings.param_hiding_disconnect,
                "param_hiding_disconnect",
            )?,
            latest_rr: format_address(
                &prefix,
                &osc_settings.param_latest_rr_int,
                "param_latest_rr_int",
            )?,
            battery_int: format_address(
                &prefix,
                &osc_settings.param_hrm_battery_int,
                "param_hrm_battery_int",
            )?,
            battery_float: format_address(
                &prefix,
                &osc_settings.param_hrm_battery_float,
                "param_hrm_battery_float",
            )?,
            rr_twitch_up: format_address(
                &prefix,
                &osc_settings.param_rr_twitch_up,
                "param_rr_twitch_up",
            )?,
            rr_twitch_down: format_address(
                &prefix,
                &osc_settings.param_rr_twitch_down,
                "param_rr_twitch_down",
            )?,
        })
    }
}

fn make_mimic_data(hr_status: &HeartRateStatus) -> HeartRateStatus {
    let mut mimic = HeartRateStatus::default();
    let jitter = rand::thread_rng().gen_range(-3..3);
    mimic.heart_rate_bpm = hr_status.heart_rate_bpm.saturating_add_signed(jitter);
    mimic.battery_level = hr_status.battery_level;
    // Add chance to fake a twitch
    mimic.twitch_up = (rand::thread_rng().gen_range(0..5)) == 0;
    mimic.twitch_down = (rand::thread_rng().gen_range(0..5)) == 0;
    mimic
}

fn form_bpm_bundle(
    hr_status: &HeartRateStatus,
    hiding_disconnect: bool,
    delay_sending_connected: bool,
    positive_float_bpm: bool,
    osc_addresses: &OscAddresses,
) -> OscBundle {
    let mut bundle = OscBundle {
        timetag: OSC_NOW,
        content: vec![],
    };

    let bpm_int_msg = OscMessage {
        addr: osc_addresses.bpm_int.clone(),
        args: vec![OscType::Int(hr_status.heart_rate_bpm as i32)],
    };

    let bpm_float_msg = OscMessage {
        addr: osc_addresses.bpm_float.clone(),
        args: vec![OscType::Float(if positive_float_bpm {
            hr_status.heart_rate_bpm as f32 / 255.0
        } else {
            (hr_status.heart_rate_bpm as f32 / 255.0) * 2.0 - 1.0
        })],
    };

    let connected = if delay_sending_connected {
        false
    } else {
        hr_status.heart_rate_bpm > 0
    };

    let connected_msg = OscMessage {
        addr: osc_addresses.connected.clone(),
        args: vec![OscType::Bool(connected)],
    };

    let hiding_disconnect_msg = OscMessage {
        addr: osc_addresses.hiding_disconnect.clone(),
        args: vec![OscType::Bool(hiding_disconnect)],
    };

    let battery_int_msg = OscMessage {
        addr: osc_addresses.battery_int.clone(),
        args: vec![OscType::Int(match hr_status.battery_level {
            BatteryLevel::Level(level) => level as i32,
            _ => 0,
        })],
    };

    let battery_float_msg = OscMessage {
        addr: osc_addresses.battery_float.clone(),
        args: vec![OscType::Float(match hr_status.battery_level {
            BatteryLevel::Level(level) => level as f32 / 100.0,
            _ => 0.0,
        })],
    };

    if hr_status.heart_rate_bpm == 0 {
        let rr_msg = OscMessage {
            addr: osc_addresses.latest_rr.clone(),
            args: vec![OscType::Int(0)],
        };
        bundle.content.push(OscPacket::Message(rr_msg));
    } else if let Some(&latest_rr) = hr_status.rr_intervals.last() {
        let rr_msg = OscMessage {
            addr: osc_addresses.latest_rr.clone(),
            args: vec![OscType::Int((latest_rr.as_secs_f32() * 1000.0) as i32)],
        };
        bundle.content.push(OscPacket::Message(rr_msg));
    }

    let twitch_up_msg = OscMessage {
        addr: osc_addresses.rr_twitch_up.clone(),
        args: vec![OscType::Bool(hr_status.twitch_up)],
    };

    let twitch_down_msg = OscMessage {
        addr: osc_addresses.rr_twitch_down.clone(),
        args: vec![OscType::Bool(hr_status.twitch_down)],
    };

    bundle.content.push(OscPacket::Message(bpm_int_msg));
    bundle.content.push(OscPacket::Message(bpm_float_msg));
    bundle.content.push(OscPacket::Message(connected_msg));
    bundle
        .content
        .push(OscPacket::Message(hiding_disconnect_msg));
    bundle.content.push(OscPacket::Message(battery_int_msg));
    bundle.content.push(OscPacket::Message(battery_float_msg));
    bundle.content.push(OscPacket::Message(twitch_up_msg));
    bundle.content.push(OscPacket::Message(twitch_down_msg));

    bundle
}
