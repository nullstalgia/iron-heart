use std::path::PathBuf;

#[cfg(windows)]
pub mod tui;

#[derive(Debug, Default)]
pub struct VrcxStartup {
    startup_path: Option<PathBuf>,
    shortcut_path: Option<PathBuf>,
}

#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
mod unix;
