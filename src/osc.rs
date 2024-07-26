use rosc::{address, encoder};
use rosc::{OscBundle, OscMessage, OscPacket, OscTime, OscType};
use std::net::{SocketAddrV4, UdpSocket};
use std::str::FromStr;
use std::sync::Arc;
use std::{env, f32, thread};
use tokio_util::sync::CancellationToken;

use log::*;

use tokio::sync::{mpsc, Mutex};
use tokio::time::{self, sleep, Duration, Instant};

use crate::app::DeviceData;
use crate::heart_rate::HeartRateStatus;
use crate::settings::OSCSettings;

const OSC_NOW: OscTime = OscTime {
    seconds: 0,
    fractional: 0,
};

fn form_bpm_bundle(hr_status: &HeartRateStatus, osc_addresses: &OSCAddresses) -> OscBundle {
    let mut bundle = OscBundle {
        timetag: OSC_NOW,
        content: vec![],
    };

    let int_hr_msg = OscMessage {
        addr: osc_addresses.int_hr.clone(),
        args: vec![OscType::Int(hr_status.heart_rate_bpm as i32)],
    };

    let float_hr_msg = OscMessage {
        addr: osc_addresses.float_hr.clone(),
        args: vec![OscType::Float(
            (hr_status.heart_rate_bpm as f32 / 255.0) * 2.0 - 1.0,
        )],
    };

    let connected_msg = OscMessage {
        addr: osc_addresses.connected.clone(),
        args: vec![OscType::Bool(hr_status.heart_rate_bpm > 0)],
    };

    if let Some(&latest_rr) = hr_status.rr_intervals.last() {
        let rr_msg = OscMessage {
            addr: osc_addresses.latest_rr.clone(),
            args: vec![OscType::Int((latest_rr * 1000.0) as i32)],
        };
        bundle.content.push(OscPacket::Message(rr_msg));
    }

    bundle.content.push(OscPacket::Message(int_hr_msg));
    bundle.content.push(OscPacket::Message(float_hr_msg));
    bundle.content.push(OscPacket::Message(connected_msg));
    //bundle.content.push(OscPacket::Message(battery_msg));

    bundle
}

fn send_bpm_bundle(
    hr_status: &HeartRateStatus,
    osc_addresses: &OSCAddresses,
    socket: &UdpSocket,
    target_addr: SocketAddrV4,
) {
    let bundle = form_bpm_bundle(hr_status, osc_addresses);
    let msg_buf = encoder::encode(&OscPacket::Bundle(bundle)).unwrap();
    socket.send_to(&msg_buf, target_addr).unwrap();
}

fn send_beat_param(beat: bool, address: &String, socket: &UdpSocket, target_addr: SocketAddrV4) {
    let msg = OscMessage {
        addr: address.to_owned(),
        args: vec![OscType::Bool(beat)],
    };

    let msg_buf = encoder::encode(&OscPacket::Message(msg)).unwrap();
    socket.send_to(&msg_buf, target_addr).unwrap();
}

struct OSCAddresses {
    beat_toggle: String,
    beat_pulse: String,
    int_hr: String,
    float_hr: String,
    connected: String,
    latest_rr: String,
    // rr_twitch_up: String,
    // rr_twitch_down: String,
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
            int_hr: format_address(&osc_settings, &osc_settings.param_bpm_int),
            float_hr: format_address(&osc_settings, &osc_settings.param_bpm_float),
            connected: format_address(&osc_settings, &osc_settings.param_hrm_connected),
            latest_rr: format_address(&osc_settings, &osc_settings.param_latest_rr_int),
        }
    }
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

    // switch view
    // let msg_buf = encoder::encode(&OscPacket::Message(OscMessage {
    //     addr: "/avatar/parameters/test".to_string(),
    //     args: vec![OscType::Int(1)],
    // }))
    // .unwrap();

    // socket.send_to(&msg_buf, target_addr).unwrap();

    let mut hr_status = HeartRateStatus::default();
    let mut toggle_beat: bool = true;

    let mut heart_beat_interval = time::interval(Duration::from_secs(1));

    // Used when BLE connection is lost, but we don't want to
    // hide the BPM display in VRChat, we'll just bounce around
    // the last known actual value.
    let mut mimic_ble_connection = false;

    let osc_addresses = OSCAddresses::new(&osc_settings);

    // let bundle = form_bpm_bundle(hr_status.clone(), osc_settings.clone());
    // let msg_buf = encoder::encode(&OscPacket::Bundle(bundle)).unwrap();
    // socket.send_to(&msg_buf, target_addr).unwrap();

    let mut locked_receiver = osc_rx_arc.lock().await;

    // TODO:
    // with hide disconnects, make it semi-natural by randomizing the value -+3
    // dont forget to do HRTwitchUp and Down

    loop {
        tokio::select! {
            hr_data = locked_receiver.recv() => {
                match hr_data {
                    Some(data) => {
                        if data.heart_rate_bpm > 0 {
                            hr_status = data;
                            mimic_ble_connection = false;
                            send_bpm_bundle(&hr_status, &osc_addresses, &socket, target_addr);
                        } else {
                            if osc_settings.hide_disconnections_pre {
                                mimic_ble_connection = true;
                            } else {
                                hr_status = data;
                                send_bpm_bundle(&hr_status, &osc_addresses, &socket, target_addr);
                            }
                        }
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
            _ = heart_beat_interval.tick() => {
                send_beat_param(toggle_beat, &osc_addresses.beat_toggle, &socket, target_addr);
                send_beat_param(true, &osc_addresses.beat_pulse, &socket, target_addr);
                sleep(Duration::from_millis(osc_settings.pulse_length_ms as u64)).await;
                send_beat_param(false, &osc_addresses.beat_pulse, &socket, target_addr);
                toggle_beat = !toggle_beat;
            }
        }
    }
    send_bpm_bundle(
        &HeartRateStatus::default(),
        &osc_addresses,
        &socket,
        target_addr,
    );
    send_beat_param(false, &osc_addresses.beat_toggle, &socket, target_addr);
    send_beat_param(false, &osc_addresses.beat_pulse, &socket, target_addr);
    // let mut msg_buf = encoder::encode(&OscPacket::Message(OscMessage {
    //     addr: format!("")
    //     args: vec![OscType::Float(x), OscType::Float(y)],
    // }))
    // .unwrap();

    // sock.send_to(&msg_buf, to_addr).unwrap();
}
