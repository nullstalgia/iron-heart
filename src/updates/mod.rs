use std::env::consts::EXE_SUFFIX;
use std::env::current_exe;
use std::path::PathBuf;

use crate::app::{App, ErrorPopup, SubState};
use crate::errors::AppError;
use crate::is_portable;
use crate::ui::table_state_scroll;

use futures_util::StreamExt;
use http::HeaderMap;
use self_update::cargo_crate_version;
use self_update::update::ReleaseAsset;
use self_update::version::bump_is_greater;
use self_update::{get_target, update::Release};
use sha2::{Digest, Sha512};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{error, info};
use tui::{UpdateCheckChoice, UpdatePromptChoice};

#[cfg(windows)]
use crossterm::event::DisableMouseCapture;
#[cfg(windows)]
use crossterm::terminal::LeaveAlternateScreen;
#[cfg(windows)]
use tui::UpdateRestartChoice;

pub mod tui;

#[derive(Debug)]
enum UpdateCommand {
    CheckForUpdate,
    DownloadUpdate,
    LaunchUpdatedApp,
}
#[derive(Debug)]
pub enum UpdateReply {
    UpToDate,
    UpdateFound(String),
    DownloadProgress(f64),
    ReadyToLaunch,
    Error(AppError),
}
#[derive(Debug)]
struct UpdateBackend {
    command_rx: mpsc::Receiver<UpdateCommand>,
    reply_tx: mpsc::Sender<UpdateReply>,
    archive_asset: Option<ReleaseAsset>,
    checksum_asset: Option<ReleaseAsset>,
    headers: HeaderMap,
    current_exe: Option<PathBuf>,
}
impl UpdateBackend {
    fn new(
        receiver: mpsc::Receiver<UpdateCommand>,
        async_sender: mpsc::Sender<UpdateReply>,
    ) -> Self {
        UpdateBackend {
            command_rx: receiver,
            reply_tx: async_sender,
            archive_asset: None,
            checksum_asset: None,
            headers: HeaderMap::new(),
            current_exe: None,
        }
    }
    async fn handle_message(&mut self, msg: UpdateCommand) {
        match msg {
            UpdateCommand::CheckForUpdate => {
                self.check_for_update().await;
            }
            UpdateCommand::DownloadUpdate => match self.update_executable().await {
                Ok(()) => self
                    .reply_tx
                    .send(UpdateReply::ReadyToLaunch)
                    .await
                    .expect("Failed to signal update download complete"),
                Err(e) => self
                    .reply_tx
                    .send(UpdateReply::Error(e))
                    .await
                    .expect("Failed to send updater error"),
            },
            UpdateCommand::LaunchUpdatedApp => match self.start_new_version() {
                Ok(()) => {
                    unreachable!()
                }
                Err(e) => self
                    .reply_tx
                    .send(UpdateReply::Error(e))
                    .await
                    .expect("Failed to send updater error"),
            },
        }
    }
    /// Streams the supplied URL's contents into the given File, returning the SHA512 hash of the received bytes as a String
    async fn download_and_verify<T: AsyncWrite + Unpin>(
        &self,
        archive_url: String,
        checksum_url: String,
        mut file: T,
    ) -> Result<(), AppError> {
        let client = reqwest::Client::new();

        let resp = client
            .get(&checksum_url)
            .headers(self.headers.to_owned())
            .send()
            .await?;
        let size = resp.content_length().unwrap_or(0);
        if !resp.status().is_success() || size == 0 {
            error!("Failed to get archive checksum!");
            return Err(AppError::HttpStatus(resp.status().as_u16()));
        }

        let content = resp.text().await?;
        // Format is `checksum *filename`
        // So we just want the first "word" in the line
        let expected = content
            .split_whitespace()
            .next()
            .ok_or(AppError::MissingChecksum)?;

        let resp = client
            .get(&archive_url)
            .headers(self.headers.to_owned())
            .send()
            .await?;
        let size = resp.content_length().unwrap_or(0);
        if !resp.status().is_success() || size == 0 {
            error!("Failed to get archive!");
            return Err(AppError::HttpStatus(resp.status().as_u16()));
        }

        let mut byte_stream = resp.bytes_stream();
        let mut downloaded: u64 = 0;
        let mut hasher = Sha512::new();

        while let Some(item) = byte_stream.next().await {
            let chunk = item?;
            tokio::io::copy(&mut chunk.as_ref(), &mut file).await?;
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;
            let percentage = downloaded as f64 / size as f64;
            let _ = self
                .reply_tx
                .try_send(UpdateReply::DownloadProgress(percentage));
        }
        let result = hasher.finalize();
        let checksum = format!("{result:x}");

        if checksum.eq(expected) {
            info!("Update checksum matches! SHA512: {expected}");
            Ok(())
        } else {
            error!(
                "Archive SHA512 checksum mismatch! Expected: {expected} != Calculated: {checksum}"
            );
            Err(AppError::BadChecksum)
        }
    }
    /// This should never return, unless an error occurs.
    fn start_new_version(&mut self) -> Result<(), AppError> {
        let current_exe = self.current_exe.take().unwrap();
        // In the happy path, this function won't return
        // since we're ending the process and replacing it with the new one
        Err(restart_process(current_exe))?;

        unreachable!()
    }
    async fn update_executable(&mut self) -> Result<(), AppError> {
        if !is_portable() {
            return Err(AppError::NotPortable);
        }
        let archive = self.archive_asset.take().expect("Missing archive asset");
        let checksum = self.checksum_asset.take().expect("Missing checksum asset");

        // A lot yoinked from
        // https://github.com/jaemk/self_update/blob/60b3c13533e731650031ee2c410f4bbb4483e845/src/update.rs#L227
        let tmp_archive_dir = tempfile::TempDir::new()?;
        let tmp_archive_path = tmp_archive_dir.path().join(&archive.name);
        let mut tmp_archive = tokio::fs::File::create(&tmp_archive_path).await?;

        info!("Temp archive location: {}", tmp_archive_path.display());

        self.headers.insert(
            http::header::ACCEPT,
            "application/octet-stream".parse().unwrap(),
        );
        self.headers.insert(
            http::header::USER_AGENT,
            "iron-heart/self-update"
                .parse()
                .expect("invalid user-agent"),
        );
        // self.headers.insert(
        //     http::header::AUTHORIZATION,
        //     (String::from("token ") + "github_pat_XYZ")
        //         .parse()
        //         .unwrap(),
        // );

        self.download_and_verify(
            archive.download_url,
            checksum.download_url,
            &mut tmp_archive,
        )
        .await?;

        tmp_archive.flush().await?;

        let bin_name = env!("CARGO_PKG_NAME");
        let bin_name = format!("{}{}", bin_name, EXE_SUFFIX);
        self.current_exe = Some(current_exe()?);

        self_update::Extract::from_source(&tmp_archive_path)
            .extract_file(tmp_archive_dir.path(), &bin_name)?;

        let new_exe = tmp_archive_dir.path().join(bin_name);

        self_replace::self_replace(new_exe)?;

        Ok(())
    }
    async fn check_for_update(&mut self) {
        let latest = tokio::task::spawn_blocking(|| -> Result<(Release, bool), AppError> {
            let bin_name = env!("CARGO_PKG_NAME");
            let current = cargo_crate_version!();
            let release = self_update::backends::github::Update::configure()
                // .auth_token("github_pat_XYZ")
                .repo_owner("nullstalgia")
                .repo_name("iron-heart")
                .bin_name(bin_name)
                .current_version(current)
                .build()?
                .get_latest_release()?;
            let newer = bump_is_greater(current, &release.version)?;
            Ok((release, newer))
        })
        .await;

        match latest {
            Ok(Ok((release, newer))) => {
                let version = release.version.clone();
                let target = get_target();
                if version.contains("pre") {
                    error!("Latest was a pre-release? Ignoring...");
                    self.command_rx.close();
                    return;
                };

                let Some((archive, checksum)) = asset_pair_for(target, &release.assets) else {
                    error!("Couldn't find SHA+Archive for given target: {target}");
                    self.command_rx.close();
                    return;
                };

                if newer {
                    self.archive_asset = Some(archive);
                    self.checksum_asset = Some(checksum);
                    self.reply_tx
                        .send(UpdateReply::UpdateFound(version))
                        .await
                        .expect("Failed to send latest release version");
                } else {
                    self.reply_tx
                        .send(UpdateReply::UpToDate)
                        .await
                        .expect("Failed to send up to date message");
                }
            }
            Ok(Err(err)) => {
                error!("Error getting latest release: {}", err);
                self.command_rx.close();
            }
            Err(err) => {
                error!("Error joining get_latest_release: {}", err);
                self.command_rx.close();
            }
        }
    }
}

/// Returns a pair of ReleaseAssets for the given target from the list of assets
///
/// Returns None if there aren't exactly two files for the given target (either there's too many or too little, we expect one checksum per archive)
///
/// Returns Assets in the order of (Archive, SHA512 Checksum)
fn asset_pair_for(target: &str, releases: &[ReleaseAsset]) -> Option<(ReleaseAsset, ReleaseAsset)> {
    let assets: Vec<&ReleaseAsset> = releases
        .iter()
        .filter(|asset| asset.name.contains(target))
        .collect();

    if assets.len() != 2 {
        return None;
    }

    // I'm gonna assume we get the items in a non-determinate order, so let's sort them ourselves.
    let (checksums, archives): (Vec<&ReleaseAsset>, Vec<&ReleaseAsset>) = assets
        .iter()
        .partition(|asset| asset.name.ends_with(".sha512"));

    // Should be symmetrical since only two total elements
    if checksums.len() != archives.len() {
        return None;
    }

    Some((archives[0].clone(), checksums[0].clone()))
}

async fn update_backend_loop(mut actor: UpdateBackend) {
    while let Some(msg) = actor.command_rx.recv().await {
        actor.handle_message(msg).await;
    }
}

#[derive(Debug)]
pub struct UpdateHandle {
    command_tx: mpsc::Sender<UpdateCommand>,
    pub reply_rx: mpsc::Receiver<UpdateReply>,
}

impl UpdateHandle {
    pub fn new() -> Self {
        let (command_tx, command_rx) = mpsc::channel(8);
        let (reply_tx, reply_rx) = mpsc::channel(8);
        let actor = UpdateBackend::new(command_rx, reply_tx);
        tokio::spawn(update_backend_loop(actor));
        Self {
            command_tx,
            reply_rx,
        }
    }
    pub fn query_latest(&self) {
        let msg = UpdateCommand::CheckForUpdate;
        self.command_tx
            .try_send(msg)
            .expect("Unable to start query for version");
    }
    pub fn download_update(&self) {
        let msg = UpdateCommand::DownloadUpdate;
        self.command_tx
            .try_send(msg)
            .expect("Unable to start query for version");
    }
    pub fn start_new_version(&self) {
        let msg = UpdateCommand::LaunchUpdatedApp;
        self.command_tx
            .try_send(msg)
            .expect("Unable to signal for new app launch");
    }
}

// Yoinked from
// https://github.com/lichess-org/fishnet/blob/eac238abbd77b7fc8cacd2d1f7c408252746e2f5/src/main.rs#L399

fn restart_process(current_exe: PathBuf) -> std::io::Error {
    exec(std::process::Command::new(current_exe).args(std::env::args_os().skip(1)))
}

#[cfg(unix)]
fn exec(command: &mut std::process::Command) -> std::io::Error {
    use std::os::unix::process::CommandExt as _;
    // Completely replace the current process image. If successful, execution
    // of the current process stops here.
    command.exec()
}

#[cfg(windows)]
fn exec(command: &mut std::process::Command) -> std::io::Error {
    use std::os::windows::process::CommandExt as _;
    // No equivalent for Unix exec() exists. So create a new independent
    // console instead and terminate the current one:
    // https://docs.microsoft.com/en-us/windows/win32/procthread/process-creation-flags
    let create_new_console = 0x0000_0010;
    match command.creation_flags(create_new_console).spawn() {
        Ok(_) => std::process::exit(libc::EXIT_SUCCESS),
        Err(err) => err,
    }
}

impl App {
    pub fn updates_enter_pressed(&mut self) {
        match self.sub_state {
            SubState::UpdateFoundPrompt => {
                // If we're not portable, we just want to open a link to the repo, let the user handle the update via however they installed it.
                let chosen_option = self.prompt_state.selected().unwrap_or(0);
                match UpdatePromptChoice::from(chosen_option as u8) {
                    UpdatePromptChoice::OpenRepository => {
                        let url = format!("{}/releases", env!("CARGO_PKG_REPOSITORY"));
                        info!("{url}");
                        if let Err(e) = opener::open_browser(url) {
                            self.handle_error_update(ErrorPopup::UserMustDismiss(format!(
                                "Failed to open app repository! {}",
                                e
                            )));
                        }
                    }
                    UpdatePromptChoice::SkipVersion => {
                        self.settings
                            .updates
                            .version_skipped
                            .clone_from(self.update_newer_version.as_ref().unwrap());
                        self.try_save_settings();
                        self.sub_state = SubState::None;
                    }
                    UpdatePromptChoice::No => {
                        self.sub_state = SubState::None;
                    }
                    UpdatePromptChoice::Yes => {
                        if !is_portable() {
                            return;
                        }
                        self.cancel_actors.cancel();
                        self.sub_state = SubState::UpdateDownloading;
                        self.updates.download_update();
                    }
                }
            }
            SubState::UpdateAllowCheckPrompt => {
                let chosen_option = self.prompt_state.selected().unwrap_or(0);
                match UpdateCheckChoice::from(chosen_option as u8) {
                    UpdateCheckChoice::No => {
                        self.sub_state = SubState::None;
                    }
                    UpdateCheckChoice::NeverAsk => {
                        self.settings.updates.allow_checking_for_updates = false;
                        self.settings.updates.update_check_prompt = false;
                        self.try_save_settings();
                        self.sub_state = SubState::None;
                    }
                    UpdateCheckChoice::Yes => {
                        self.settings.updates.allow_checking_for_updates = true;
                        self.settings.updates.update_check_prompt = false;
                        self.try_save_settings();
                        self.auto_update_prompt();
                    }
                }
            }
            #[cfg(windows)]
            SubState::LaunchUpdatePrompt => {
                crossterm::terminal::disable_raw_mode().unwrap();
                crossterm::execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture)
                    .unwrap();
                let chosen_option = self.prompt_state.selected().unwrap_or(0);
                match UpdateRestartChoice::from(chosen_option as u8) {
                    UpdateRestartChoice::Yes => self.updates.start_new_version(),
                    UpdateRestartChoice::No => std::process::exit(libc::EXIT_SUCCESS),
                }
            }
            _ => {}
        }
    }
    pub fn updates_scroll(&mut self, up: bool) {
        match self.sub_state {
            SubState::UpdateFoundPrompt => {
                // If we're not portable, we just want to open a link to the repo, let the user handle the update via however they installed it.
                table_state_scroll(up, &mut self.prompt_state, 4);
                // Preventing the Download option from being chosen on non-portable builds
                if self.prompt_state.selected() == Some(0) && !is_portable() {
                    if up {
                        self.prompt_state.select(Some(3));
                    } else {
                        self.prompt_state.select(Some(1));
                    }
                }
            }
            SubState::UpdateAllowCheckPrompt => table_state_scroll(up, &mut self.prompt_state, 3),
            #[cfg(windows)]
            SubState::LaunchUpdatePrompt => table_state_scroll(up, &mut self.prompt_state, 2),
            _ => {}
        }
    }
}
