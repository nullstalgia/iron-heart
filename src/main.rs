#[macro_use]
extern crate lazy_static;
use crate::viewer::viewer;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{error::Error, io};

use log::*;

mod app;
mod company_codes;
mod heart_rate;
mod heart_rate_dummy;
mod heart_rate_measurement;
mod heart_rate_websocket;
mod logging;
mod osc;
mod panic_handler;
mod scan;
mod settings;
mod structs;
mod utils;
mod viewer;
mod widgets;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = app::App::new();
    let had_error = app.error_message.is_some();
    let log_path = std::env::current_exe()
        .expect("Failed to get executable path")
        .with_extension("log");
    let log_str = log_path
        .to_str()
        .expect("Failed to convert log path to string");
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
            .file_loop(log_str, fast_log::consts::LogSize::MB(1))
            .level(log_level)
            .format(log_format)
            .chan_len(Some(1000000)),
    )
    .expect("Failed to initialize log writer");

    info!("Starting app...");

    if !had_error {
        // Creates default config and adds any missing fields
        // (will remove fields that aren't declared in settings.rs)
        app.save_settings()?;
        if app.settings.dummy.enabled {
            app.start_dummy_thread().await;
        } else if app.settings.websocket.enabled {
            app.start_websocket_thread().await;
        } else {
            app.start_bluetooth_event_thread().await;
            app.start_logging_thread().await;
        }
        app.start_osc_thread().await;
    }
    // Main app loop
    viewer(&mut terminal, &mut app).await?;
    app.join_threads().await;

    info!("Shutting down gracefully...");
    log::logger().flush();
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    Ok(())
}
