pub mod ble;
pub mod dummy;
pub mod measurement;
pub mod websocket;

mod twitcher;

use std::time::Duration;

use chrono::{DateTime, Local};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BatteryLevel {
    #[default]
    Unknown,
    NotReported,
    Level(u8),
}

impl From<BatteryLevel> for u8 {
    fn from(level: BatteryLevel) -> Self {
        match level {
            BatteryLevel::Level(battery) => battery,
            _ => 0,
        }
    }
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
    pub timestamp: DateTime<Local>,
}

// Only used as a backup if the HRM doesn't support
// sending RR intervals
// (Or when mimicking)
pub fn rr_from_bpm(bpm: u16) -> Duration {
    // Make sure it's at least 1 to prevent a potential divide by zero
    let bpm = bpm.max(1);
    Duration::from_secs_f32(60.0 / bpm as f32)
}

// #[derive(Error, Debug)]
// pub enum MonitorError {
//     #[error("Device is missing HR service")]
//     BLEError(#[from] btleplug::Error),
// }
