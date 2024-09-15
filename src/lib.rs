#[macro_use]
extern crate lazy_static;

use argh::FromArgs;
use errors::AppError;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, path::PathBuf};
use tokio::fs::create_dir;

use log::*;

use crate::app::App;
use event::{Event, EventHandler};
use handler::handle_key_events;
use std::error;
use tui::Tui;

#[cfg(not(any(debug_assertions, feature = "portable")))]
use directories::BaseDirs;

mod activities;
mod app;
mod company_codes;
mod errors;
mod heart_rate;
mod logging;
mod macros;
mod osc;
mod panic_handler;
mod scan;
mod settings;
mod structs;
mod utils;
mod widgets;

mod event;
mod handler;
mod tui;
mod ui;

#[derive(FromArgs)]
/// Optional command line arguments
pub struct ArgConfig {
    /// specify config file path
    #[argh(option, short = 'c')]
    config_override: Option<PathBuf>,
}

/// Application result type.
pub type AppResult<T> = std::result::Result<T, Box<dyn error::Error>>;

pub async fn run_tui(mut arg_config: ArgConfig) -> AppResult<()> {
    let working_directory = determine_working_directory().ok_or(AppError::WorkDir)?;
    arg_config.config_override = arg_config.config_override.map(|p| {
        p.canonicalize()
            .expect("Failed to build full supplied config path")
    });
    info!("Working directory: {}", working_directory.display());
    if !working_directory.exists() {
        create_dir(&working_directory)
            .await
            .map_err(|e| AppError::CreateDir {
                path: working_directory.clone(),
                source: e,
            })?;
    }
    std::env::set_current_dir(&working_directory).expect("Failed to change working directory");
    let mut app = App::build(arg_config);

    // Initialize the terminal user interface.
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    let events = EventHandler::new(100);
    let mut tui = Tui::new(terminal, events);
    tui.init()?;

    let had_error = app.error_message.is_some();
    let log_name = std::env::current_exe()?.with_extension("log");
    let log_path = working_directory.with_file_name(&log_name);
    let log_path = log_path
        .to_str()
        .expect("Failed to convert log path to &str");
    let log_level = app.settings.get_log_level();
    let log_format = if log_level <= LevelFilter::Info || had_error {
        // Default format
        fast_log::FastLogFormat::new()
    } else {
        // Show line number
        fast_log::FastLogFormat::new().set_display_line_level(LevelFilter::Trace)
    };
    fast_log::init(
        fast_log::Config::new()
            .file_loop(log_path, fast_log::consts::LogSize::MB(1))
            .level(log_level)
            .format(log_format)
            .chan_len(Some(1000000)),
    )
    .expect("Failed to initialize fast_log");

    info!("Starting app...");

    app.init();

    // Start the main loop.
    while !app.cancel_app.is_cancelled() {
        // Render the user interface.
        tui.draw(&mut app)?;
        // Handle Crossterm events.
        match tui.events.next().await? {
            Event::Tick => app.term_tick(),
            Event::Key(key_event) => handle_key_events(&mut app, key_event)?,
        }
        // Dispatch BLE/HR/OSC messages
        app.main_loop().await;
    }
    // After while loop closes
    app.join_threads().await;

    info!("Shutting down gracefully...");
    log::logger().flush();

    // Reset the terminal.
    tui.exit()?;
    Ok(())
}

/// Returns the directory that logs, config, and other files should be placed in by default.
// The rules for how it determines the directory is as follows:
// If the app is built with the portable feature, it will just return it's parent directory.
// If there is a config file present adjacent to the executable, the executable's parent path is returned.
// Otherwise, it will return the `directories` `config_dir` output.
//
// Debug builds are always portable. Release builds can optionally have the "portable" feature enabled.
fn determine_working_directory() -> Option<PathBuf> {
    let portable = is_portable();
    let exe_path = std::env::current_exe().expect("Failed to get executable path");
    let exe_parent = exe_path
        .parent()
        .expect("Couldn't get parent dir of executable")
        .to_path_buf();
    let config_path = exe_path.with_extension("toml");

    if portable || config_path.exists() {
        Some(exe_parent)
    } else {
        get_user_dir()
    }
}

#[cfg(any(debug_assertions, feature = "portable"))]
fn is_portable() -> bool {
    true
}

#[cfg(not(any(debug_assertions, feature = "portable")))]
fn is_portable() -> bool {
    false
}

#[cfg(any(debug_assertions, feature = "portable"))]
fn get_user_dir() -> Option<PathBuf> {
    None
}

#[cfg(not(any(debug_assertions, feature = "portable")))]
fn get_user_dir() -> Option<PathBuf> {
    if let Some(base_dirs) = BaseDirs::new() {
        let mut config_dir = base_dirs.config_dir().to_owned();
        config_dir.push(env!("CARGO_PKG_NAME"));
        Some(config_dir)
    } else {
        None
    }
}
