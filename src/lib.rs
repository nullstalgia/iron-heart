#![deny(unused_must_use)]
#[macro_use]
extern crate lazy_static;

use args::TopLevelCmd;
use errors::AppError;
use ratatui::{backend::CrosstermBackend, Terminal};
use self_update::cargo_crate_version;
use std::{io, path::PathBuf};
use tokio::fs::create_dir;
use tokio_util::sync::CancellationToken;

use crate::app::App;
use event::{Event, EventHandler};
use handler::handle_key_events;
use std::error;

use tui::Tui;

use rolling_file::{BasicRollingFileAppender, RollingConditionBasic};
use tracing::info;
use tracing_subscriber::{filter, prelude::*};
use tracing_subscriber::{fmt::time::ChronoLocal, layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(not(any(debug_assertions, feature = "portable")))]
use directories::BaseDirs;

pub mod args;
pub mod errors;

pub mod heart_rate;
mod activities;
mod app;
mod company_codes;
mod logging;
mod macros;
mod osc;
mod panic_handler;
mod scan;
mod settings;
mod structs;
mod updates;
mod utils;
mod vrcx;
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
    if !working_directory.exists() {
        create_dir(&working_directory)
            .await
            .map_err(|e| AppError::CreateDir {
                path: working_directory.clone(),
                source: e,
            })?;
    }
    std::env::set_current_dir(&working_directory).expect("Failed to change working directory");
    let log_name = std::env::current_exe()?
        .with_extension("log")
        .file_name()
        .expect("Couldn't build log path!")
        .to_owned();
    // let console = console_subscriber::spawn();
    let file_appender = BasicRollingFileAppender::new(
        log_name,
        RollingConditionBasic::new().max_size(1024 * 1024 * 5),
        2,
    )
    .unwrap();
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let time_fmt = ChronoLocal::new("%Y-%m-%d %H:%M:%S%.6f".to_owned());
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        // .pretty()
        .with_file(false)
        .with_ansi(false)
        .with_target(true)
        .with_timer(time_fmt)
        .with_line_number(true)
        .with_filter(filter::LevelFilter::DEBUG);
    let (fmt_layer, reload_handle) = tracing_subscriber::reload::Layer::new(fmt_layer);
    // Allow everything through but limit lnk to just info, since it spits out a bit too much when reading shortcuts
    let env_filter = tracing_subscriber::EnvFilter::new("trace,lnk=info");
    tracing_subscriber::registry()
        // .with(console)
        .with(env_filter)
        .with(fmt_layer)
        .init();

    let mut app = App::build(&arg_config, None);

    // Initialize the terminal user interface.
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    let events = EventHandler::new(100);
    let mut tui = Tui::new(terminal, events);
    tui.init()?;

    info!("Starting app... v{}", cargo_crate_version!());

    // Starting off at DEBUG, and setting to whatever user has defined
    reload_handle.modify(|layer| *layer.filter_mut() = app.settings.get_log_level())?;

    app.init(&arg_config).await;

    // Only when running TUI
    app.first_time_setup(&arg_config).await;

    // Start the main loop.
    while !app.cancel_app.is_cancelled() {
        // Render the user interface.
        tui.draw(&mut app)?;
        tokio::select! {
            // Handle Crossterm events.
            val = tui.events.next() => {
                match val {
                    Ok(event) => {
                        match event {
                            Event::Tick => app.term_tick(),
                            Event::Key(key_event) => handle_key_events(&mut app, key_event)?,
                            Event::Resize => tui.autoresize()?,
                        }
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
            // Handle BLE Manager Events/Update UI with HR info
            data = app.app_receivers() => app.app_handlers(data).await
        }
    }
    // After while loop closes
    app.join_threads().await;

    info!("Shutting down gracefully...");

    // Reset the terminal.
    tui.exit()?;
    Ok(())
}

pub async fn run_headless(
    arg_config: TopLevelCmd,
    parent_token: CancellationToken,
) -> Result<(), AppError> {
    // let working_directory = determine_working_directory().ok_or(AppError::WorkDir)?;
    let mut app = App::build(&arg_config, Some(parent_token));

    assert_eq!(app.error_message, None);

    info!("Loaded config from: {}", app.config_path.display());

    info!("Starting app... v{}", cargo_crate_version!());

    app.init(&arg_config).await;

    // Since there's no UI to dismiss errors, just close the app
    // if the actors aren't happy
    while !app.cancel_app.is_cancelled() && !app.cancel_actors.is_cancelled() {
        assert_eq!(app.error_message, None);
        tokio::select! {
            data = app.app_receivers() => app.app_handlers(data).await
        }
    }
    info!("Joining...");
    // After while loop closes
    app.join_threads().await;

    info!("Shutting down gracefully...");

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
