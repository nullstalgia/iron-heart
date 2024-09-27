use std::path::PathBuf;

#[cfg(windows)]
pub mod tui;

#[cfg(windows)]
mod windows;

#[cfg(windows)]
#[derive(Debug, Default)]
pub struct VrcxStartup {
    startup_path: Option<PathBuf>,
    shortcut_path: Option<PathBuf>,
}

#[cfg(not(windows))]
mod unix;

#[cfg(not(windows))]
pub struct VrcxStartup;
