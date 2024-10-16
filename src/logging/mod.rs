use crate::app::{AppUpdate, ErrorPopup};
use crate::broadcast;

use crate::settings::{MiscSettings, PrometheusSettings};

use file::FileLoggingActor;
use prometheus::PrometheusLoggingActor;
use tokio::sync::broadcast::{Receiver as BReceiver, Sender as BSender};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

mod file;
mod prometheus;

pub async fn file_logging_thread(
    mut broadcast_rx: BReceiver<AppUpdate>,
    broadcast_tx: BSender<AppUpdate>,
    initial_activity: u8,
    misc_settings: MiscSettings,
    cancel_token: CancellationToken,
) {
    if !misc_settings.log_sessions_to_csv && !misc_settings.write_bpm_to_file {
        info!("No file logging was enabled! Shutting down thread.");
        return;
    }

    let mut logging = FileLoggingActor::new(initial_activity, misc_settings);

    info!("Logging thread started!");

    if let Err(e) = logging.rx_loop(&mut broadcast_rx, cancel_token).await {
        error!("File Logging error: {e}");
        let message = "File Logging error.";
        broadcast!(broadcast_tx, ErrorPopup::detailed(message, e));
    }
}

pub async fn prometheus_logging_thread(
    mut broadcast_rx: BReceiver<AppUpdate>,
    broadcast_tx: BSender<AppUpdate>,
    initial_activity: u8,
    prometheus_settings: PrometheusSettings,
    cancel_token: CancellationToken,
) {
    if !prometheus_settings.enabled {
        info!("Prometheus wasn't enabled! Shutting down thread");
        return;
    }

    let mut logging = match PrometheusLoggingActor::build(initial_activity, prometheus_settings) {
        Ok(Some(prom)) => prom,
        Ok(None) => {
            info!("Prometheus: No metrics specified, shutting down thread");
            return;
        }
        Err(e) => {
            let message = "Failed to build Prometheus sender";
            broadcast!(broadcast_tx, ErrorPopup::detailed(message, e));
            return;
        }
    };

    info!("Prometheus thread started!");

    if let Err(e) = logging.rx_loop(&mut broadcast_rx, cancel_token).await {
        error!("Prometheus error: {e}");
        let message = "Prometheus error:";
        broadcast!(broadcast_tx, ErrorPopup::detailed(message, e));
    }
}
