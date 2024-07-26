use rosc::{address, encoder};
use rosc::{OscBundle, OscMessage, OscPacket, OscTime, OscType};
use std::net::{SocketAddrV4, UdpSocket};
use std::str::FromStr;
use std::sync::Arc;
use std::{env, f32, thread};

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

fn form_bpm_bundle(hr_status: HeartRateStatus, osc_settings: OSCSettings) -> OscBundle {
    let mut bundle = OscBundle {
        timetag: OSC_NOW,
        content: vec![],
    };

    let int_hr_msg = OscMessage {
        addr: format!(
            "{}/{}",
            osc_settings.address_prefix, osc_settings.param_bpm_int
        ),
        args: vec![OscType::Int(hr_status.heart_rate_bpm as i32)],
    };

    let float_hr_msg = OscMessage {
        addr: format!(
            "{}/{}",
            osc_settings.address_prefix, osc_settings.param_bpm_float
        ),
        args: vec![OscType::Float(
            (hr_status.heart_rate_bpm as f32 / 255.0) * 2.0 - 1.0,
        )],
    };

    let connected_msg = OscMessage {
        addr: format!(
            "{}/{}",
            osc_settings.address_prefix, osc_settings.param_hrm_connected
        ),
        args: vec![OscType::Bool(hr_status.heart_rate_bpm > 0)],
    };

    if let Some(&latest_rr) = hr_status.rr_intervals.last() {
        let rr_msg = OscMessage {
            addr: format!(
                "{}/{}",
                osc_settings.address_prefix, osc_settings.param_latest_rr_int
            ),
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

fn send_beat_param(beat: bool, address: String, sock: &UdpSocket, target_addr: SocketAddrV4) {
    let msg = OscMessage {
        addr: address,
        args: vec![OscType::Bool(beat)],
    };

    let msg_buf = encoder::encode(&OscPacket::Message(msg)).unwrap();
    sock.send_to(&msg_buf, target_addr).unwrap();
}

pub async fn osc_thread(
    osc_rx_arc: Arc<Mutex<mpsc::UnboundedReceiver<HeartRateStatus>>>,
    osc_settings: OSCSettings,
) {
    // let host_addr =
    //     SocketAddrV4::from_str(&format!("{}:{}", osc_settings.host_ip, osc_settings.port)).unwrap();
    let target_addr =
        SocketAddrV4::from_str(&format!("{}:{}", osc_settings.target_ip, osc_settings.port))
            .unwrap();
    // TODO Add error handling
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

    // switch view
    let msg_buf = encoder::encode(&OscPacket::Message(OscMessage {
        addr: "/avatar/parameters/test".to_string(),
        args: vec![OscType::Int(1)],
    }))
    .unwrap();

    socket.send_to(&msg_buf, target_addr).unwrap();

    let mut hr_status = HeartRateStatus::default();
    let mut toggle_beat: bool = true;

    let mut heart_beat_interval = time::interval(Duration::from_secs(1));

    let param_beat_toggle = format!(
        "{}/{}",
        osc_settings.address_prefix, osc_settings.param_beat_toggle
    );
    let param_beat_pulse = format!(
        "{}/{}",
        osc_settings.address_prefix, osc_settings.param_beat_pulse
    );

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
                        if (data.heart_rate_bpm > 0) {
                            hr_status = data;
                            let bundle = form_bpm_bundle(hr_status.clone(), osc_settings.clone());
                            let msg_buf = encoder::encode(&OscPacket::Bundle(bundle)).unwrap();
                            socket.send_to(&msg_buf, target_addr).unwrap();
                        } else {
                            if osc_settings.dont_show_disconnections_pre {
                                hr_status.heart_rate_bpm = 0;
                                hr_status.rr_intervals = vec![];
                            } else {
                                let bundle = form_bpm_bundle(hr_status.clone(), osc_settings.clone());
                                let msg_buf = encoder::encode(&OscPacket::Bundle(bundle)).unwrap();
                                socket.send_to(&msg_buf, target_addr).unwrap();
                            }
                        }
                    },
                    None => {
                        error!("OSC: Channel closed");
                        break;
                    },
                }
            }
            _ = heart_beat_interval.tick() => {
                send_beat_param(toggle_beat, param_beat_toggle.clone(), &socket, target_addr);
                send_beat_param(true, param_beat_pulse.clone(), &socket, target_addr);
                sleep(Duration::from_millis(osc_settings.pulse_length_ms as u64)).await;
                send_beat_param(false, param_beat_pulse.clone(), &socket, target_addr);
                toggle_beat = !toggle_beat;
            }
        }
    }

    // let mut msg_buf = encoder::encode(&OscPacket::Message(OscMessage {
    //     addr: format!("")
    //     args: vec![OscType::Float(x), OscType::Float(y)],
    // }))
    // .unwrap();

    // sock.send_to(&msg_buf, to_addr).unwrap();
}
