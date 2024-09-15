use crate::app::{AppUpdate, DeviceUpdate, ErrorPopup};
use crate::errors::AppError;
use crate::heart_rate_measurement::parse_hrm;
use crate::structs::DeviceInfo;

use btleplug::api::Peripheral;
use futures::StreamExt;
use log::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::{Receiver as BReceiver, Sender as BSender};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

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
    // Twitches are calculated by HR sources so that
    // all listeners see twitches at the same time
    pub twitch_up: bool,
    pub twitch_down: bool,
}

// Only used as a backup if the HRM doesn't support
// sending RR intervals
// (Or when mimicking)
pub fn rr_from_bpm(bpm: u16) -> Duration {
    Duration::from_secs_f32(60.0 / bpm as f32)
}

// #[derive(Error, Debug)]
// pub enum MonitorError {
//     #[error("Device is missing HR service")]
//     BLEError(#[from] btleplug::Error),
// }
