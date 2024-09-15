use crate::heart_rate::HeartRateStatus;
use rand::Rng;
use rosc::encoder;
use rosc::{OscBundle, OscMessage, OscPacket, OscType};
use std::f32;

use super::addresses::OscAddresses;
use super::OSC_NOW;

use std::net::{SocketAddrV4, UdpSocket};

use crate::errors::AppError;

pub(super) fn send_raw_hr_status(
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

pub(super) fn send_raw_beat_params(
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

pub(super) fn make_mimic_data(hr_status: &HeartRateStatus) -> HeartRateStatus {
    let mut mimic = HeartRateStatus::default();
    let jitter = rand::thread_rng().gen_range(-3..3);
    mimic.heart_rate_bpm = hr_status.heart_rate_bpm.saturating_add_signed(jitter);
    mimic.battery_level = hr_status.battery_level;
    // Add chance to fake a twitch
    mimic.twitch_up = (rand::thread_rng().gen_range(0..5)) == 0;
    mimic.twitch_down = (rand::thread_rng().gen_range(0..5)) == 0;
    mimic
}

pub(super) fn form_bpm_bundle(
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

    let battery_level: u8 = hr_status.battery_level.into();

    let battery_int_msg = OscMessage {
        addr: osc_addresses.battery_int.clone(),
        args: vec![OscType::Int(battery_level as i32)],
    };

    let battery_float_msg = OscMessage {
        addr: osc_addresses.battery_float.clone(),
        args: vec![OscType::Float(battery_level as f32 / 100.0)],
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
