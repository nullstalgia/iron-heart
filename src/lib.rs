#![deny(unused_must_use)]
#[macro_use]
extern crate lazy_static;

use args::TopLevelCmd;
use errors::AppError;
use fast_log::FastLogFormat;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io,
    path::{Path, PathBuf},
};
use tokio::fs::create_dir;
use tokio_util::sync::CancellationToken;

use log::*;

use crate::app::App;
use event::{Event, EventHandler};
use handler::handle_key_events;
use std::error;
use tui::Tui;

#[cfg(not(any(debug_assertions, feature = "portable")))]
use directories::BaseDirs;

pub mod args;
pub mod errors;

mod activities;
mod app;
mod company_codes;
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

/// Application result type.
//pub type AppResult<T> = color_eyre::eyre::Result<T>;
pub type AppResult<T> = std::result::Result<T, Box<dyn error::Error>>;

pub async fn run_tui(mut arg_config: TopLevelCmd) -> AppResult<()> {
    let working_directory = determine_working_directory().ok_or(AppError::WorkDir)?;
    arg_config.config_override = arg_config.config_override.map(|p| {
        p.canonicalize()
            .expect("Failed to build full supplied config path")
        // Can also fail if doesn't exist. Need to decide how to handle.
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
    let mut app = App::build(&arg_config, None);

    // Initialize the terminal user interface.
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    let events = EventHandler::new(100);
    let mut tui = Tui::new(terminal, events);
    tui.init()?;

    let (log_path, log_level, log_format) = log_config(&app, &working_directory)?;
    let log_path = log_path
        .to_str()
        .expect("Failed to convert log path to &str");
    fast_log::init(
        fast_log::Config::new()
            .file_loop(log_path, fast_log::consts::LogSize::MB(1))
            .level(log_level)
            .format(log_format)
            .chan_len(Some(1000000)),
    )
    .expect("Failed to initialize fast_log");

    info!("Starting app...");

    app.init(&arg_config);

    // Start the main loop.
    while !app.cancel_app.is_cancelled() {
        // Render the user interface.
        tui.draw(&mut app)?;
        // Handle Crossterm events.
        match tui.events.next().await? {
            Event::Tick => app.term_tick(),
            Event::Key(key_event) => handle_key_events(&mut app, key_event)?,
        }
        // Handle BLE Manager Events/Update UI with HR info
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

pub async fn run_headless(
    arg_config: TopLevelCmd,
    parent_token: CancellationToken,
) -> Result<(), AppError> {
    let working_directory = determine_working_directory().ok_or(AppError::WorkDir)?;

    let mut app = App::build(&arg_config, Some(parent_token));

    let (_, log_level, log_format) = log_config(&app, &working_directory)?;

    // assert_eq!("a", std::env::current_dir().unwrap().to_str().unwrap());

    fast_log::init(
        fast_log::Config::new()
            .console()
            .level(log_level)
            .format(log_format)
            .chan_len(Some(1000000)),
    )
    .expect("Failed to initialize fast_log");

    assert_eq!(app.error_message, None);

    info!("Loaded config from: {}", app.config_path.display());

    info!("Starting app...");

    app.init(&arg_config);

    // Start the main loop.
    while !app.cancel_app.is_cancelled() {
        assert_eq!(app.error_message, None);
        // Handle BLE Manager Events
        app.main_loop().await;
        // Since there's no UI to dismiss errors, just close the app
        // if the actors aren't happy
        if app.cancel_actors.is_cancelled() {
            info!("Actors cancelled!");
            app.cancel_app.cancel();
        }
    }
    info!("Joining...");
    // After while loop closes
    app.join_threads().await;

    info!("Shutting down gracefully...");
    log::logger().flush();

    Ok(())
}

fn log_config(
    app: &App,
    working_directory: &Path,
) -> Result<(PathBuf, LevelFilter, FastLogFormat), AppError> {
    let had_error = app.error_message.is_some();
    let log_name = std::env::current_exe()?.with_extension("log");
    let log_path = working_directory.with_file_name(&log_name);
    let log_level = app.settings.get_log_level();
    let log_format = if log_level <= LevelFilter::Info || had_error {
        // Default format
        fast_log::FastLogFormat::new()
    } else {
        // Show line number
        fast_log::FastLogFormat::new().set_display_line_level(LevelFilter::Trace)
    };

    Ok((log_path, log_level, log_format))
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
