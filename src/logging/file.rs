use crate::app::AppUpdate;
use crate::errors::AppError;
use crate::heart_rate::HeartRateStatus;
use crate::settings::MiscSettings;

use csv_async::AsyncSerializer;
use serde::Serialize;
use std::error::Error;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs::{File, create_dir};
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast::Receiver as BReceiver;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

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
    Activity: u8,
}

pub(super) struct FileLoggingActor {
    misc_settings: MiscSettings,
    csv_writer: Option<AsyncSerializer<File>>,
    csv_path: Option<PathBuf>,
    txt_path: Option<PathBuf>,
    files_initialized: bool,
    // Loop-specific vars
    last_rr: Duration,
    activity: u8,
}

impl FileLoggingActor {
    pub(super) const fn new(initial_activity: u8, misc_settings: MiscSettings) -> Self {
        Self {
            misc_settings,
            csv_writer: None,
            csv_path: None,
            txt_path: None,
            last_rr: Duration::from_secs(0),
            files_initialized: false,
            activity: initial_activity,
        }
    }
    pub(super) async fn rx_loop(
        &mut self,
        broadcast_rx: &mut BReceiver<AppUpdate>,
        cancel_token: CancellationToken,
    ) -> Result<(), AppError> {
        loop {
            tokio::select! {
                heart_rate_status = broadcast_rx.recv() => {
                    match heart_rate_status {
                        Ok(AppUpdate::HeartRateStatus(data)) => {
                            self.handle_data(data).await?;
                        },
                        Ok(AppUpdate::ActivitySelected(index)) => {
                            // Dunno if I want to trigger a CSV save here
                            self.activity = index;
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
                () = cancel_token.cancelled() => {
                    info!("Logging thread shutting down");
                    return Ok(());
                }
            }
        }
    }
    async fn initialize_files(&mut self) -> Result<(), AppError> {
        let txt_path = self.misc_settings.bpm_file_path.clone();

        let csv_folder = PathBuf::from(self.misc_settings.log_sessions_csv_path.clone());
        let mut csv_file_path = csv_folder.clone();
        let csv_file_name = format!(
            "{}{}.csv",
            CSV_FILE_PREFIX,
            chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
        );
        csv_file_path.push(csv_file_name);

        if self.misc_settings.log_sessions_to_csv {
            if !csv_folder.exists() {
                create_dir(&csv_folder)
                    .await
                    .map_err(|e| AppError::CreateDir {
                        path: csv_folder,
                        source: e,
                    })?;
            }
            self.csv_writer = Some(AsyncSerializer::from_writer(
                File::options()
                    .create(true)
                    .truncate(true)
                    .read(true)
                    .write(true)
                    .open(&csv_file_path)
                    .await
                    .map_err(|e| AppError::CreateFile {
                        path: csv_file_path,
                        source: e,
                    })?,
            ));
        }
        if self.misc_settings.write_bpm_to_file || self.misc_settings.write_rr_to_file {
            self.txt_path = Some(txt_path.as_str().into());
            let path_clone = txt_path.clone();
            let attempt_create_file = async move || {
                File::options()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(&path_clone)
                    .await
            };
            try_until_err_count_is(5, attempt_create_file)
                .await
                .map_err(|e| AppError::CreateFile {
                    path: txt_path.into(),
                    source: e,
                })?;
            // Honestly this probably doesn't benefit from being in a BufWriter due to being polled every whole second or so.
            // self.txt_writer = Some(BufWriter::new(file));
            // self.txt_writer = Some(file);
        }
        self.files_initialized = true;
        Ok(())
    }
    async fn handle_data(&mut self, heart_rate_status: HeartRateStatus) -> Result<(), AppError> {
        if heart_rate_status.heart_rate_bpm == 0 {
            return Ok(());
        }
        if !self.files_initialized {
            self.initialize_files().await?;
        }
        let reported_rr = heart_rate_status
            .rr_intervals
            .last()
            .unwrap_or(&self.last_rr);
        let rr_as_millis = reported_rr.as_millis();

        if let Some(csv_writer) = &mut self.csv_writer {
            let timestamp = heart_rate_status.timestamp.format("%Y-%m-%d %H:%M:%S%.6f");
            let csv_data = CsvData {
                Timestamp: timestamp.to_string(),
                BPM: heart_rate_status.heart_rate_bpm,
                RR: rr_as_millis as u16,
                Battery: heart_rate_status.battery_level.into(),
                TwitchUp: u8::from(heart_rate_status.twitch_up),
                TwitchDown: u8::from(heart_rate_status.twitch_down),
                Activity: self.activity,
            };
            csv_writer.serialize(csv_data).await?;
            csv_writer.flush().await.map_err(|e| AppError::WriteFile {
                path: self.csv_path.as_ref().unwrap().to_owned(),
                source: e,
            })?;
        }

        let write_bpm = self.misc_settings.write_bpm_to_file;
        let write_rr = self.misc_settings.write_rr_to_file;
        if let Some(txt_path) = &self.txt_path {
            assert!(write_bpm || write_rr);
            let path_clone = txt_path.clone();
            let write_to_file = async move || -> Result<(), std::io::Error> {
                let mut txt_file = File::options()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(&path_clone)
                    .await?;
                let mut buf = Vec::new();
                if write_bpm {
                    writeln!(buf, "{}", heart_rate_status.heart_rate_bpm)?;
                }
                if write_rr {
                    writeln!(buf, "{rr_as_millis}")?;
                }

                // txt_file.set_len(0).await?;
                // txt_file.seek(tokio::io::SeekFrom::Start(0)).await?;
                txt_file.write_all(&buf).await?;
                txt_file.flush().await?;
                // txt_writer.sync_data().await?;
                Ok(())
            };

            try_until_err_count_is(5, write_to_file)
                .await
                .map_err(|e| AppError::WriteFile {
                    path: txt_path.to_owned(),
                    source: e,
                })?;
        }
        self.last_rr = *reported_rr;

        Ok(())
    }
}

async fn try_until_err_count_is<
    T,
    E: Error + Send + Sync,
    A: AsyncFnMut() -> Result<T, E> + Send + Sync,
>(
    max_err: u8,
    mut fallible: A,
) -> Result<T, E> {
    assert!(max_err > 0);
    let mut err_cnt = 0;
    loop {
        match fallible().await {
            Ok(o) => return Ok(o),
            Err(e) => {
                err_cnt += 1;
                error!("err_cnt: {err_cnt}/{max_err}, {e}");
                if err_cnt >= max_err {
                    return Err(e);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
