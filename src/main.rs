#[macro_use]
extern crate lazy_static;
use crate::viewer::viewer;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use simplelog::*;
use std::{error::Error, io};

mod app;
mod company_codes;
mod heart_rate;
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

    let log_path = std::env::current_exe()
        .expect("Failed to get executable path")
        .with_extension("log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect(format!("Failed to open log file: {:?}", log_path).as_str());
    WriteLogger::init(app.settings.get_log_level(), Config::default(), log_file)
        .expect("Failed to initialize log writer");

    // Try to create a default config file if it doesn't exist
    app.save_settings()?;

    app.start_osc_thread().await;
    app.start_bluetooth_event_thread().await;
    // Main app loop
    viewer(&mut terminal, &mut app).await?;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    Ok(())
}
