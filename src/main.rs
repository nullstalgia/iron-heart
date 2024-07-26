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
mod heart_rate_measurement;
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
            .file(log_str)
            .level(log_level)
            .format(log_format)
            .chan_len(Some(1000000)),
    )
    .expect("Failed to initialize log writer");

    info!("Starting app...");

    if !had_error {
        // Try to create a default config file if it doesn't exist
        app.save_settings()?;
        app.start_osc_thread().await;
        app.start_bluetooth_event_thread().await;
        debug!("Started OSC and Bluetooth CentralEvent threads");
    }
    // Main app loop
    viewer(&mut terminal, &mut app).await?;

    // Shutting down gracefully
    log::logger().flush();
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    Ok(())
}
