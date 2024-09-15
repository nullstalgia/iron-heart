use crate::app::{AppUpdate, ErrorPopup};
use crate::errors::AppError;
use crate::heart_rate::{BatteryLevel, HeartRateStatus};
use crate::settings::MiscSettings;

use csv_async::AsyncSerializer;
use log::*;
use serde::Serialize;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::{create_dir, File};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::{Receiver as BReceiver, Sender as BSender};
use tokio_util::sync::CancellationToken;

const CSV_FILE_PREFIX: &str = "nih-";

#[allow(non_snake_case)]
#[derive(Debug, Serialize)]
struct CsvData {
    Timestamp: String,
    BPM: u16,
    RR: u16,
    Battery: u8,
    TwitchUp: u8,
    TwitchDown: u8,
}

struct FileLoggingActor {
    misc_settings: MiscSettings,
    cancel_token: CancellationToken,
    csv_writer: Option<AsyncSerializer<File>>,
    txt_writer: Option<BufWriter<File>>,
    // Loop-specific vars
    last_rr: Duration,
}

impl FileLoggingActor {
    async fn build(
        misc_settings: MiscSettings,
        cancel_token: CancellationToken,
    ) -> Result<Self, AppError> {
        let txt_path = misc_settings.bpm_file_path.clone();

        let csv_folder = PathBuf::from(misc_settings.log_sessions_csv_path.clone());
        let mut csv_file_path = csv_folder.clone();
        let csv_file_name = format!(
            "{}{}.csv",
            CSV_FILE_PREFIX,
            chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
        );
        csv_file_path.push(csv_file_name);

        let mut csv_writer = None;
        if misc_settings.log_sessions_to_csv {
            if !csv_folder.exists() {
                create_dir(&csv_folder)
                    .await
                    .map_err(|e| AppError::CreateDir {
                        path: csv_folder,
                        source: e,
                    })?;
            }
            csv_writer = Some(AsyncSerializer::from_writer(
                File::create(&csv_file_path)
                    .await
                    .map_err(|e| AppError::CreateFile {
                        path: csv_file_path,
                        source: e,
                    })?,
            ));
        }
        let mut txt_writer = None;
        if misc_settings.write_bpm_to_file {
            let file = File::create(&txt_path)
                .await
                .map_err(|e| AppError::CreateFile {
                    path: PathBuf::from(txt_path),
                    source: e,
                })?;
            txt_writer = Some(BufWriter::new(file));
        }
        Ok(Self {
            misc_settings,
            cancel_token,
            csv_writer,
            txt_writer,
            last_rr: Duration::from_secs(0),
        })
    }
    async fn rx_loop(&mut self, broadcast_rx: &mut BReceiver<AppUpdate>) -> Result<(), AppError> {
        loop {
            tokio::select! {
                heart_rate_status = broadcast_rx.recv() => {
                    match heart_rate_status {
                        Ok(AppUpdate::HeartRateStatus(data)) => {
                            self.handle_data(data).await?;
                        },
                        Ok(_) => {},
                        Err(RecvError::Closed) => {
                            error!("File Logging: Channel closed");
                            return Ok(());
                        },
                        Err(RecvError::Lagged(count)) => {
                            warn!("File Logging: Lagged! Missed {count} messages");
                        }
                    }
                }
                _ = self.cancel_token.cancelled() => {
                    info!("Logging thread shutting down");
                    return Ok(());
                }
            }
        }
    }
    async fn handle_data(&mut self, heart_rate_status: HeartRateStatus) -> Result<(), AppError> {
        if heart_rate_status.heart_rate_bpm > 0 {
            let reported_rr = heart_rate_status
                .rr_intervals
                .last()
                .unwrap_or(&self.last_rr);

            if let Some(csv_writer) = &mut self.csv_writer {
                let csv_data = CsvData {
                    Timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    BPM: heart_rate_status.heart_rate_bpm,
                    RR: reported_rr.as_millis() as u16,
                    Battery: match heart_rate_status.battery_level {
                        BatteryLevel::Level(battery) => battery,
                        _ => 0,
                    },
                    TwitchUp: heart_rate_status.twitch_up as u8,
                    TwitchDown: heart_rate_status.twitch_down as u8,
                };
                csv_writer.serialize(csv_data).await?;
                csv_writer.flush().await?;
            }
            if let Some(txt_writer) = &mut self.txt_writer {
                let txt_output = if self.misc_settings.write_rr_to_file {
                    format!(
                        "{}\n{}\n",
                        heart_rate_status.heart_rate_bpm,
                        reported_rr.as_millis()
                    )
                } else {
                    format!("{}\n", heart_rate_status.heart_rate_bpm)
                };
                txt_writer.seek(tokio::io::SeekFrom::Start(0)).await?;
                txt_writer.write_all(txt_output.as_bytes()).await?;
                txt_writer.flush().await?;
            }
            self.last_rr = *reported_rr;
        }
        Ok(())
    }
}

pub async fn file_logging_thread(
    mut broadcast_rx: BReceiver<AppUpdate>,
    broadcast_tx: BSender<AppUpdate>,
    misc_settings: MiscSettings,
    cancel_token: CancellationToken,
) {
    if !misc_settings.log_sessions_to_csv && !misc_settings.write_bpm_to_file {
        info!("No file logging was enabled! Shutting down thread.");
        return;
    }

    let mut logging = match FileLoggingActor::build(misc_settings, cancel_token).await {
        Ok(logging) => logging,
        Err(e) => {
            let message = format!("Failed to set up File Logging. {e}");
            broadcast_tx
                .send(AppUpdate::Error(ErrorPopup::Fatal(message)))
                .expect("Failed to send error message");
            return;
        }
    };

    info!("Logging thread started!");

    if let Err(e) = logging.rx_loop(&mut broadcast_rx).await {
        error!("File Logging error: {e}");
        let message = format!("File Logging error: {e}");
        broadcast_tx
            .send(AppUpdate::Error(ErrorPopup::Fatal(message)))
            .expect("Failed to send error message");
    }
}
