#[cfg(windows)]
pub mod tui;

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::VrcxStartup;

#[cfg(not(windows))]
mod unix;

#[cfg(not(windows))]
pub use unix::VrcxStartup;
