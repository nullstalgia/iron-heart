use crate::heart_rate::{BatteryLevel, HeartRateStatus};
use crate::settings::MiscSettings;

use csv_async::AsyncSerializer;
use log::*;
use serde::Serialize;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::{create_dir, File};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;

const CSV_FILE_PREFIX: &str = "nih-";

#[allow(non_snake_case)]
#[derive(Debug, Serialize)]
struct CsvData {
    Timestamp: String,
    BPM: u16,
    RR: u16,
    Battery: u8,
}

const RR_IGNORE_COUNT: usize = 5;

pub async fn logging_thread(
    logging_rx_arc: Arc<Mutex<mpsc::UnboundedReceiver<HeartRateStatus>>>,
    misc_settings: MiscSettings,
    shutdown_token: CancellationToken,
) {
    info!("Logging thread started!");
    let mut locked_reciever = logging_rx_arc.lock().await;

    let exe_path = env::current_exe().expect("Failed to get executable path");

    let mut last_rr = Duration::from_secs(0);
    // I've noticed the first few RR intervals after a reconnect can have
    // garbage data. This is a simple way to ignore them.
    let mut rr_cooldown = 0;

    let mut txt_path = exe_path.parent().unwrap().to_path_buf();
    txt_path.push(misc_settings.bpm_file_path);

    let mut csv_folder = exe_path.parent().unwrap().to_path_buf();
    csv_folder.push(misc_settings.log_sessions_csv_path);
    let mut csv_file_path = csv_folder.clone();
    let csv_file_name = format!(
        "{}{}.csv",
        CSV_FILE_PREFIX,
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    csv_file_path.push(csv_file_name);

    let mut csv_writer: Option<AsyncSerializer<File>> = None;
    if misc_settings.log_sessions_to_csv {
        if !csv_folder.exists() {
            create_dir(csv_folder)
                .await
                .expect("Failed to create CSV log folder!");
        }
        csv_writer = Some(AsyncSerializer::from_writer(
            File::create(&csv_file_path)
                .await
                .expect("Failed to create CSV for session!"),
        ));
    }
    let mut txt_writer: Option<BufWriter<File>> = None;
    if misc_settings.write_bpm_to_file {
        let file = File::create(&txt_path)
            .await
            .expect("Failed to create BPM file!");
        txt_writer = Some(BufWriter::new(file));
    }

    loop {
        tokio::select! {
            Some(heart_rate_status) = locked_reciever.recv() => {
                if heart_rate_status.heart_rate_bpm > 0 {
                    debug!("{:?}", heart_rate_status);
                    let reported_rr = if rr_cooldown == 0 {
                        heart_rate_status.rr_intervals.last().unwrap_or(&last_rr)
                    } else {
                        rr_cooldown -= 1;
                        &last_rr
                    };
                    if let Some(csv_writer) = &mut csv_writer {
                        let csv_data = CsvData {
                            Timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                            BPM: heart_rate_status.heart_rate_bpm,
                            RR: reported_rr.as_millis() as u16,
                            Battery: match heart_rate_status.battery_level {
                                BatteryLevel::Level(battery) => battery,
                                _ => 0,
                            },
                        };
                        csv_writer.serialize(csv_data).await.expect("Failed to write to CSV file");
                        csv_writer.flush().await.expect("Failed to flush CSV file");
                    }
                    if let Some(txt_writer) = &mut txt_writer {
                        let txt_output = if misc_settings.write_rr_to_file {
                            format!("{}\n{}\n", heart_rate_status.heart_rate_bpm, reported_rr.as_millis())
                        } else {
                            format!("{}\n", heart_rate_status.heart_rate_bpm)
                        };
                        txt_writer
                            .seek(tokio::io::SeekFrom::Start(0))
                            .await
                            .expect("Failed to seek to start of BPM file");
                        txt_writer.write(txt_output.as_bytes()).await.expect("Failed to write to BPM file");
                        txt_writer.flush().await.expect("Failed to flush BPM file");
                    }
                    last_rr = *reported_rr;
                } else {
                    rr_cooldown = RR_IGNORE_COUNT;
                }
            }
            _ = shutdown_token.cancelled() => {
                info!("Logging thread shutting down");
                break;
            }
        }
    }
}
