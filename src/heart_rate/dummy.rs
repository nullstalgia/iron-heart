use super::{rr_from_bpm, BatteryLevel, HeartRateStatus};
use crate::app::{AppUpdate, ErrorPopup};
use crate::broadcast;
use crate::settings::DummySettings;

use log::*;
use std::time::Duration;

use tokio::time;
use tokio_util::sync::CancellationToken;

use tokio::sync::broadcast::Sender as BSender;

pub async fn dummy_thread(
    broadcast_tx: BSender<AppUpdate>,
    dummy_settings: DummySettings,
    seconds_override: Option<f32>,
    vhs_mode: bool,
    cancel_token: CancellationToken,
) {
    let bpm_updates_per_sec = seconds_override.unwrap_or(dummy_settings.bpm_speed);
    let bpm_update_interval = Duration::from_secs_f32(1.0 / (bpm_updates_per_sec));
    let mut bpm_update_interval = time::interval(bpm_update_interval);
    let low_bpm = dummy_settings.low_bpm;
    let high_bpm = dummy_settings.high_bpm;
    let mut loops_before_dc = dummy_settings.loops_before_dc;

    let mut loops: u16 = 0;
    let mut positive_direction = true;
    let mut hr_status = HeartRateStatus {
        heart_rate_bpm: low_bpm.saturating_sub(1),
        battery_level: BatteryLevel::Level(100),
        ..Default::default()
    };

    let mut dummy_tick = || {
        hr_status.timestamp = chrono::Local::now();
        if vhs_mode {
            loops_before_dc = 0;
        }
        let bound = if positive_direction {
            hr_status.heart_rate_bpm += 1;
            high_bpm
        } else {
            hr_status.heart_rate_bpm -= 1;
            low_bpm
        };
        hr_status.rr_intervals = vec![rr_from_bpm(hr_status.heart_rate_bpm)];
        if hr_status.heart_rate_bpm == bound {
            positive_direction = !positive_direction;
            loops += 1;
            if loops > loops_before_dc {
                loops = 0;
            }
        }
        if loops == loops_before_dc && loops_before_dc != 0 {
            broadcast!(
                broadcast_tx,
                ErrorPopup::Intermittent(format!(
                    "Simulating lost connection ({:.0} seconds left)",
                    bound.abs_diff(hr_status.heart_rate_bpm) as f32 / dummy_settings.bpm_speed
                ),)
            );
        } else {
            broadcast!(broadcast_tx, hr_status.clone());
        }
    };

    // TODO Need to sync this with the VHS recording better now, hm. Maybe on keypress?
    // I'll wait until it's needed...
    // if vhs_mode {
    //     const DUMMY_PREFILL_AMOUNT: u8 = 120;
    //     let mut dummy_wait =
    //         time::interval(Duration::from_secs_f32(3.0 / DUMMY_PREFILL_AMOUNT as f32));
    //     for _ in 0..DUMMY_PREFILL_AMOUNT {
    //         dummy_tick();
    //         dummy_wait.tick().await;
    //     }
    // }

    loop {
        tokio::select! {
            _ = bpm_update_interval.tick() => {
                dummy_tick();
            }
            _ = cancel_token.cancelled() => {
                info!("Shutting down Dummy thread!");
                break;
            }
        }
    }
}
