use log::*;
use rand::Rng;
use rosc::encoder;
use rosc::{OscBundle, OscMessage, OscPacket, OscTime, OscType};
use std::net::{SocketAddrV4, UdpSocket};
use std::str::FromStr;
use std::sync::Arc;
use std::{env, f32, thread};
use tokio::sync::{mpsc, Mutex};
use tokio::time::{self, sleep, Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::heart_rate::{BatteryLevel, HeartRateStatus};
use crate::settings::OSCSettings;

const OSC_NOW: OscTime = OscTime {
    seconds: 0,
    fractional: 0,
};

fn form_bpm_bundle(
    hr_status: &HeartRateStatus,
    hiding_disconnect: bool,
    osc_addresses: &OSCAddresses,
) -> OscBundle {
    let mut bundle = OscBundle {
        timetag: OSC_NOW,
        content: vec![],
    };

    let hr_int_msg = OscMessage {
        addr: osc_addresses.hr_int.clone(),
        args: vec![OscType::Int(hr_status.heart_rate_bpm as i32)],
    };

    let hr_float_msg = OscMessage {
        addr: osc_addresses.hr_float.clone(),
        args: vec![OscType::Float(
            (hr_status.heart_rate_bpm as f32 / 255.0) * 2.0 - 1.0,
        )],
    };

    let connected_msg = OscMessage {
        addr: osc_addresses.connected.clone(),
        args: vec![OscType::Bool(hr_status.heart_rate_bpm > 0)],
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
            BatteryLevel::Level(level) => (level as f32 / 100.0),
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

    bundle.content.push(OscPacket::Message(hr_int_msg));
    bundle.content.push(OscPacket::Message(hr_float_msg));
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

fn send_bpm_bundle(
    hr_status: &HeartRateStatus,
    hiding_disconnect: bool,
    osc_addresses: &OSCAddresses,
    socket: &UdpSocket,
    target_addr: SocketAddrV4,
) {
    let bundle = form_bpm_bundle(hr_status, hiding_disconnect, osc_addresses);
    let msg_buf = encoder::encode(&OscPacket::Bundle(bundle)).unwrap();
    socket.send_to(&msg_buf, target_addr).unwrap();
}

fn send_beat_params(
    pulse_edge: bool,
    toggle_beat: bool,
    osc_addresses: &OSCAddresses,
    socket: &UdpSocket,
    target_addr: SocketAddrV4,
) {
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

    let msg_buf = encoder::encode(&OscPacket::Bundle(bundle)).unwrap();
    socket.send_to(&msg_buf, target_addr).unwrap();
}

struct OSCAddresses {
    beat_toggle: String,
    beat_pulse: String,
    hr_int: String,
    hr_float: String,
    connected: String,
    hiding_disconnect: String,
    latest_rr: String,
    battery_int: String,
    battery_float: String,
    rr_twitch_up: String,
    rr_twitch_down: String,
}

fn format_address(osc_settings: &OSCSettings, param: &str) -> String {
    let mut address = format!("{}/{}", osc_settings.address_prefix, param);
    while let Some(pos) = address.find("//") {
        address.replace_range(pos..pos + 2, "/");
    }
    address
}

impl OSCAddresses {
    fn new(osc_settings: &OSCSettings) -> Self {
        OSCAddresses {
            beat_toggle: format_address(&osc_settings, &osc_settings.param_beat_toggle),
            beat_pulse: format_address(&osc_settings, &osc_settings.param_beat_pulse),
            hr_int: format_address(&osc_settings, &osc_settings.param_bpm_int),
            hr_float: format_address(&osc_settings, &osc_settings.param_bpm_float),
            connected: format_address(&osc_settings, &osc_settings.param_hrm_connected),
            hiding_disconnect: format_address(&osc_settings, &osc_settings.param_hiding_disconnect),
            latest_rr: format_address(&osc_settings, &osc_settings.param_latest_rr_int),
            battery_int: format_address(&osc_settings, &osc_settings.param_hrm_battery_int),
            battery_float: format_address(&osc_settings, &osc_settings.param_hrm_battery_float),
            rr_twitch_up: format_address(&osc_settings, &osc_settings.param_rr_twitch_up),
            rr_twitch_down: format_address(&osc_settings, &osc_settings.param_rr_twitch_down),
        }
    }
}

// Only used as a backup if the HRM doesn't support
// sending RR intervals
// (Or when mimicking)
fn rr_from_bpm(bpm: u16) -> Duration {
    Duration::from_secs_f32(60.0 / bpm as f32)
}

fn mimic_hr_activity(hr_status: &HeartRateStatus) -> HeartRateStatus {
    let mut mimic = HeartRateStatus::default();
    let jitter = rand::thread_rng().gen_range(-3..3);
    mimic.heart_rate_bpm = hr_status.heart_rate_bpm.saturating_add_signed(jitter);
    mimic.battery_level = hr_status.battery_level;
    // Add chance to fake a twitch
    mimic.twitch_up = (rand::thread_rng().gen_range(0..5)) == 0;
    mimic.twitch_down = (rand::thread_rng().gen_range(0..5)) == 0;
    mimic
}

pub async fn osc_thread(
    osc_rx_arc: Arc<Mutex<mpsc::UnboundedReceiver<HeartRateStatus>>>,
    osc_settings: OSCSettings,
    shutdown_token: CancellationToken,
) {
    let target_addr =
        SocketAddrV4::from_str(&format!("{}:{}", osc_settings.target_ip, osc_settings.port))
            .expect("Invalid target IP address!");
    // TODO Add error handling
    let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind to UDP socket!");

    let osc_addresses = OSCAddresses::new(&osc_settings);

    // Initalize
    send_bpm_bundle(
        &HeartRateStatus::default(),
        false,
        &osc_addresses,
        &socket,
        target_addr,
    );
    send_beat_params(false, false, &osc_addresses, &socket, target_addr);

    // Always the most recent data from the monitor
    let mut hr_status = HeartRateStatus::default();
    let mut toggle_beat: bool = true;

    let mut use_real_rr = false;
    let mut latest_rr = Duration::from_secs(1);
    let mut heart_beat_interval = time::interval(latest_rr);
    let beat_pulse_duration = Duration::from_millis(osc_settings.pulse_length_ms as u64);
    let mut pulse_edge = false;

    // Used when BLE connection is lost, but we don't want to
    // hide the BPM display in VRChat, we'll just bounce around
    // the last known actual value until we reconnect or time out.
    let mut hide_ble_disconnection = false;
    let mut mimic_update_interval = time::interval(Duration::from_secs(7));

    let mut disconnected_at = Instant::now();

    let max_hide_disconnection =
        Duration::from_secs(osc_settings.max_hide_disconnection_sec as u64);

    let mut locked_receiver = osc_rx_arc.lock().await;

    // TODO:
    // Don't allow showing 0 on the display, just hide it until we've put in a real value

    // Maybe option for twitches to be a toggle and/or pulse?
    // Current implementation is a weird mix of both, but is simple to implement
    loop {
        tokio::select! {
            hr_data = locked_receiver.recv() => {
                match hr_data {
                    Some(data) => {
                        if data.heart_rate_bpm > 0 {
                            hr_status = data;
                            if let Some(new_rr) = hr_status.rr_intervals.last() {
                                latest_rr = *new_rr;
                                // Mark that we know we'll get real RR intervals
                                use_real_rr = true;
                            } else if !use_real_rr {
                                latest_rr = rr_from_bpm(hr_status.heart_rate_bpm);
                            }
                            hide_ble_disconnection = false;
                        } else {
                            if osc_settings.hide_disconnections {
                                if !hide_ble_disconnection {
                                    hide_ble_disconnection = true;
                                    disconnected_at = Instant::now();
                                }
                            } else {
                                hr_status = data;
                            }
                        }
                        send_bpm_bundle(&hr_status, hide_ble_disconnection, &osc_addresses, &socket, target_addr);
                    },
                    None => {
                        error!("OSC: Channel closed");
                        break;
                    },
                }
            }
            _ = shutdown_token.cancelled() => {
                info!("Shutting down OSC thread!");
                break;
            }
            // This interval flip/flops between the RR duration and the pulse duration
            // to allow sending a pulse without a blocking sleep() call
            _ = heart_beat_interval.tick() => {
                if hr_status.heart_rate_bpm > 0 {
                    if hide_ble_disconnection && disconnected_at.elapsed() > max_hide_disconnection {
                        pulse_edge = false;
                    } else if !pulse_edge {
                        // Rising edge
                        pulse_edge = true;
                        toggle_beat = !toggle_beat;
                        heart_beat_interval = time::interval(beat_pulse_duration);
                        heart_beat_interval.reset();
                    } else {
                        // Falling edge
                        pulse_edge = false;
                        let new_interval = latest_rr.saturating_sub(beat_pulse_duration);
                        heart_beat_interval = time::interval(new_interval);
                        heart_beat_interval.reset();
                    }
                    send_beat_params(pulse_edge, toggle_beat, &osc_addresses, &socket, target_addr);
                }
            }
            _ = mimic_update_interval.tick() => {
                if hide_ble_disconnection {
                    if disconnected_at.elapsed() > max_hide_disconnection {
                        hr_status = HeartRateStatus::default();
                        send_bpm_bundle(&hr_status, false, &osc_addresses, &socket, target_addr);
                        send_beat_params(false, false, &osc_addresses, &socket, target_addr);
                    } else if hr_status.heart_rate_bpm > 0 {
                        let mimic = mimic_hr_activity(&hr_status);
                        send_bpm_bundle(&mimic, hide_ble_disconnection, &osc_addresses, &socket, target_addr);
                    }
                }
            }
        }
    }
    send_bpm_bundle(
        &HeartRateStatus::default(),
        false,
        &osc_addresses,
        &socket,
        target_addr,
    );
    send_beat_params(false, false, &osc_addresses, &socket, target_addr);
}
