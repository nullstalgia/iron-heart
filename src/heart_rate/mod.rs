pub mod ble;
pub mod dummy;
pub mod measurement;
pub mod websocket;

use std::time::Duration;

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
