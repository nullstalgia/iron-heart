use crate::app::AppUpdate;
use crate::errors::AppError;
use crate::heart_rate::HeartRateStatus;
use crate::settings::MiscSettings;

use csv_async::AsyncSerializer;
use serde::Serialize;
use std::io::SeekFrom;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs::{File, create_dir};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::broadcast::Receiver as BReceiver;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

const CSV_FILE_PREFIX: &str = "nih-";
const BPM_FILE_TEMP_EXT: &str = "tmp";

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
    csv_writer: Option<AsyncSerializer<tokio::fs::File>>,
    csv_path: Option<PathBuf>,
    txt: Option<TxtMethod>,
    files_initialized: bool,
    // Loop-specific vars
    last_rr: Duration,
    activity: u8,
}

enum TxtMethod {
    /// Keep a handle open to the file, replacing the contents with each update
    Handle(File),

    /// Move a new file on top of the old one
    Move(PathBuf),
}

macro_rules! try_until_err_count_is {
    ($max_err:literal, async $run:block) => {{
        let max_err: u8 = const {
            assert!($max_err > 0);
            $max_err
        };
        let mut err_cnt = 0;
        loop {
            let meow = async { $run }.await;
            match meow {
                Ok(o) => break Ok(o),
                Err(e) => {
                    err_cnt += 1;
                    error!("err_cnt: {err_cnt}/{max_err}, {e}");
                    if err_cnt >= max_err {
                        break Err(e);
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }};
    ($err_max:literal, $run:expr) => {
        compile_error!("unimplemented")
    };
}

impl FileLoggingActor {
    pub(super) const fn new(initial_activity: u8, misc_settings: MiscSettings) -> Self {
        Self {
            misc_settings,
            csv_writer: None,
            csv_path: None,
            txt: None,
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
                tokio::fs::File::options()
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
            let keep_open = self.misc_settings.bpm_file_keep_open;
            let dest_path: PathBuf = txt_path.clone().into();

            let initial_path = if keep_open {
                dest_path.clone()
            } else {
                dest_path.with_added_extension(BPM_FILE_TEMP_EXT)
            };

            let file = try_until_err_count_is!(5, async {
                File::options()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(&initial_path)
                    .await
            })
            .map_err(|e| AppError::CreateFile {
                path: initial_path.clone(),
                source: e,
            })?;

            self.txt = Some(if keep_open {
                TxtMethod::Handle(file)
            } else {
                try_until_err_count_is!(5, async {
                    tokio::fs::rename(&initial_path, &dest_path).await
                })
                .map_err(|e| AppError::CreateFile {
                    path: dest_path.clone(),
                    source: e,
                })?;
                TxtMethod::Move(dest_path)
            });
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
        let mut file_opt = None;
        let mut rename_args = None;
        let file_opt: Option<&mut File> = match &mut self.txt {
            Some(TxtMethod::Handle(handle)) => Some(handle),
            Some(TxtMethod::Move(dest)) => {
                let temp_path = dest.with_added_extension(BPM_FILE_TEMP_EXT);

                let file = try_until_err_count_is!(5, async {
                    File::options()
                        .create(true)
                        .truncate(true)
                        .write(true)
                        .open(&temp_path)
                        .await
                })
                .map_err(|e| AppError::CreateFile {
                    path: temp_path.clone(),
                    source: e,
                })?;
                rename_args = Some((temp_path, dest.to_owned()));
                Some(file_opt.insert(file))
            }
            None => None,
        };
        if let Some(file) = file_opt {
            assert!(write_bpm || write_rr);

            try_until_err_count_is!(5, async {
                let mut buf = Vec::new();
                if write_bpm {
                    writeln!(buf, "{}", heart_rate_status.heart_rate_bpm)?;
                }
                if write_rr {
                    writeln!(buf, "{rr_as_millis}")?;
                }

                file.set_len(0).await?;
                file.seek(SeekFrom::Start(0)).await?;
                file.write_all(&buf).await?;
                file.flush().await?;
                if let Some((from, to)) = &rename_args {
                    tokio::fs::rename(from, to).await?;
                }
                // txt_writer.sync_data().await?;
                Result::<(), std::io::Error>::Ok(())
            })
            .map_err(|e| AppError::WriteFile {
                path: self.misc_settings.bpm_file_path.clone().into(),
                source: e,
            })?;
        }
        self.last_rr = *reported_rr;

        Ok(())
    }
}
