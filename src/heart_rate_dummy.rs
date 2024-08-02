use crate::app::{DeviceData, ErrorPopup};
use crate::heart_rate::{BatteryLevel, HeartRateStatus};
use crate::osc::rr_from_bpm;
use crate::settings::DummySettings;

use log::*;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time;
use tokio_util::sync::CancellationToken;

pub async fn start_dummy_thread(
    hr_tx: mpsc::UnboundedSender<DeviceData>,
    dummy_settings: DummySettings,
    shutdown_token: CancellationToken,
) {
    let bpm_update_per_sec = Duration::from_secs_f32(1.0 / (dummy_settings.bpm_speed));
    let mut bpm_update_interval = time::interval(bpm_update_per_sec);
    let low_bpm = dummy_settings.low_bpm;
    let high_bpm = dummy_settings.high_bpm;
    let loops_before_dc = dummy_settings.loops_before_dc;

    let mut loops: u16 = 0;
    let mut positive_direction = true;
    let mut hr_status = HeartRateStatus::default();
    hr_status.heart_rate_bpm = low_bpm.saturating_sub(1);
    hr_status.battery_level = BatteryLevel::Level(100);

    loop {
        tokio::select! {
            _ = bpm_update_interval.tick() => {
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
                    hr_tx
                        .send(DeviceData::Error(ErrorPopup::Intermittent(
                            "Simulating lost connection".to_string(),
                        )))
                        .expect("Failed to send error message");
                } else {
                    hr_tx.send(DeviceData::HeartRateStatus(hr_status.clone())).expect("Failed to send dummy message");
                }
            }
            _ = shutdown_token.cancelled() => {
                info!("Shutting down Dummy thread!");
                break;
            }
        }
    }
}
