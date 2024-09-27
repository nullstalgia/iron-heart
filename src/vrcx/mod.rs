use directories::BaseDirs;
use log::{debug, warn};
use std::{
    env::current_exe,
    path::{Path, PathBuf},
};
use tokio::fs::read_dir;

use crate::errors::AppError;

pub mod tui;

#[derive(Debug, Default)]
pub struct VrcxStartup {
    startup_path: Option<PathBuf>,
    shortcut_path: Option<PathBuf>,
}

impl VrcxStartup {
    pub async fn init(&mut self) -> Result<(), AppError> {
        self.startup_path = get_vrcx_startup_path();
        self.shortcut_path = find_shortcut(&self.startup_path).await?;
        Ok(())
    }

    pub fn vrcx_installed(&self) -> bool {
        self.startup_path.is_some()
    }

    pub fn shortcut_exists(&self) -> bool {
        self.shortcut_path.is_some()
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.startup_path.as_ref()
    }

    pub fn create_shortcut(&self) -> Result<(), AppError> {
        let startup_path = self
            .startup_path
            .as_ref()
            .expect("Couldn't get VRCX startup folder");
        // Eh, VRCX seems to make the folder even when the app launcher is disabled.
        // Plus it lets me avoid making this fn async so enter_pressed() can stay non-async
        // if !startup_path.exists() {
        //     create_dir(startup_path)
        //         .await
        //         .map_err(|e| AppError::CreateDir {
        //             path: startup_path.to_owned(),
        //             source: e,
        //         })?;
        // }
        // TODO: Look into pkg-rs's bin_name, would it be better to not query std::env::current_exe constantly?
        let exe_path = current_exe()?;
        let exe_name = exe_path.file_name().unwrap().to_str().unwrap();
        // Using the dumb lnk error adapter for now
        let shortcut =
            mslnk::ShellLink::new(&exe_path).map_err(|err| AppError::Lnk(format!("{err:?}")))?;

        let shortcut_name = format!("{} - Shortcut.lnk", exe_name);

        let mut shortcut_path = startup_path.clone();
        shortcut_path.push(shortcut_name);

        shortcut
            .create_lnk(shortcut_path)
            .map_err(|err| AppError::Lnk(format!("{err:?}")))?;

        Ok(())
    }
}

/// Returns a PathBuf to the shortcut associated with the running executable, if it exists
async fn find_shortcut(startup_path: &Option<PathBuf>) -> Result<Option<PathBuf>, AppError> {
    if startup_path.is_none() {
        return Ok(None);
    }

    let exe_name = executable_name_lowercase().expect("Couldn't get lowercase exe name");

    let startup_path = startup_path.as_ref().unwrap();
    // Return early if VCRX is installed but no startup folder exists
    if !startup_path.exists() {
        return Ok(None);
    }

    let mut dir = read_dir(startup_path).await?;

    while let Some(shortcut) = dir.next_entry().await? {
        if shortcut.file_type().await?.is_file() {
            let path = shortcut.path();
            let result = check_shortcut(&path).await;
            match result {
                Ok(is_our_shortcut) => {
                    if is_our_shortcut {
                        return Ok(Some(path));
                    }
                }
                Err(e) => {
                    let shortcut_name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(|name| name.to_lowercase())
                        .unwrap();
                    if shortcut_name.contains(&exe_name) {
                        return Err(e);
                    } else {
                        // Hm, bad shortcut but likely not for us?
                        continue;
                    }
                }
            }
        }
    }

    Ok(None)
}

/// Checks if the shortcut at a given Path resolves to the current executable
async fn check_shortcut(shortcut_path: &Path) -> Result<bool, AppError> {
    if let Some(ext) = shortcut_path.extension() {
        if ext.to_ascii_lowercase() != "lnk" {
            return Ok(false);
        }
    } else {
        return Ok(false);
    }
    let shortcut =
        lnk::ShellLink::open(shortcut_path).map_err(|err| AppError::Lnk(format!("{err:?}")))?;

    let mut shortcut_target = PathBuf::new();

    // Try to build path with working dir + relative
    if let Some(relative_path) = shortcut.relative_path() {
        if let Some(working_dir) = shortcut.working_dir() {
            shortcut_target.push(working_dir);
        }
        shortcut_target.push(relative_path);
    } else if let Some(link_info) = shortcut.link_info() {
        // I hate Windows Shortcuts...
        if let Some(local_base_path) = link_info.local_base_path() {
            shortcut_target.push(local_base_path);
            if !link_info.common_path_suffix().is_empty() {
                shortcut_target.push(link_info.common_path_suffix());
            }
        } else if let Some(local_base_path) = link_info.local_base_path_unicode() {
            shortcut_target.push(local_base_path);
            if let Some(suffix) = link_info.common_path_suffix_unicode() {
                shortcut_target.push(suffix);
            }
        }
    }

    if shortcut_target == PathBuf::new() {
        warn!("No path was resolved from: {shortcut:#?}");
        return Err(AppError::Lnk(
            "No path was resolved from shortcut! Maybe send in your logs with an Issue?"
                .to_string(),
        ));
    }

    let absolute_path = shortcut_target.canonicalize()?;
    let our_path = current_exe()?.canonicalize()?;
    if absolute_path == our_path {
        debug!("Shortcut found at {}!", shortcut_path.to_string_lossy());
        Ok(true)
    } else {
        debug!("Not our shortcut at {}", shortcut_path.to_string_lossy());
        debug!(
            "{} != {}",
            absolute_path.to_string_lossy(),
            our_path.to_string_lossy()
        );
        Ok(false)
    }
}

/// Returns None if the `Roaming/VRCX` folder doesn't exist
///
/// Returns Some(PathBuf) of `VRCX/startup` if `VRCX` exists, regardless of `startup` existing
fn get_vrcx_startup_path() -> Option<PathBuf> {
    if let Some(base_dirs) = BaseDirs::new() {
        let mut vrcx_path = base_dirs.data_dir().to_owned();
        vrcx_path.push("VRCX");

        if vrcx_path.exists() {
            vrcx_path.push("startup");
            return Some(vrcx_path);
        }
    }

    None
}

/// Returns the name of the running executable, lowercase and sans extension
fn executable_name_lowercase() -> Option<String> {
    current_exe()
        .ok()?
        .file_stem()?
        .to_str()
        .map(|name| name.to_lowercase())
}