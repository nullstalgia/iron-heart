use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::{
    event::DisableMouseCapture,
    execute,
    terminal::{disable_raw_mode, LeaveAlternateScreen},
};
use futures::SinkExt;
use ratatui::backend::Backend;
use ratatui::layout::Alignment;
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Terminal,
};
use std::error::Error;
use std::panic;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::app::{App, DeviceData};
use crate::heart_rate::HeartRateData;
use crate::structs::DeviceInfo;
use crate::utils::centered_rect;
use crate::widgets::detail_table::detail_table;
use crate::widgets::device_table::device_table;
use crate::widgets::heart_rate_display::heart_rate_display;
use crate::widgets::info_table::info_table;
use crate::widgets::inspect_overlay::inspect_overlay;

/// Displays the detected Bluetooth devices in a table and handles the user input.
/// The user can navigate the table, pause the scanning, and quit the application.
/// The detected devices are received through the provided `mpsc::Receiver`.
pub async fn viewer<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<(), Box<dyn Error>> {
    // Defining a custom panic hook to reset the terminal properties
    panic::set_hook(Box::new(|panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        eprintln!("{}", panic_info);
    }));

    app.table_state.select(Some(0));

    loop {
        // Draw UI
        terminal.draw(|f| {
            app.frame_count = f.count();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Percentage(70),
                        Constraint::Percentage(25),
                        Constraint::Percentage(5),
                    ]
                    .as_ref(),
                )
                .split(f.size());

            let device_binding = &DeviceInfo::default();
            let selected_device = app
                .discovered_devices
                .get(app.table_state.selected().unwrap_or(0))
                .unwrap_or(device_binding);

            // Draw the device table
            let device_table = device_table(app.table_state.selected(), &app.discovered_devices);
            f.render_stateful_widget(device_table, chunks[0], &mut app.table_state);

            // Draw the detail table
            let detail_table = detail_table(selected_device);
            f.render_widget(detail_table, chunks[1]);

            // Draw the info table
            app.frame_count += 1;
            let info_table: ratatui::widgets::Table<'_> = info_table(
                app.ble_scan_paused.load(Ordering::SeqCst),
                &app.is_loading_characteristics,
                &app.frame_count,
            );
            f.render_widget(info_table, chunks[2]);

            // Draw the inspect overlay
            if app.inspect_view {
                let area = centered_rect(60, 60, f.size());
                let inspect_overlay = inspect_overlay(
                    &app.selected_characteristics,
                    app.inspect_overlay_scroll,
                    area.height,
                );
                f.render_widget(Clear, area);
                f.render_widget(inspect_overlay, area);
            }

            // Draw the heart rate overlay
            if app.heart_rate_display {
                let area = centered_rect(80, 80, f.size());
                let heart_rate_overlay = heart_rate_display(&app.heart_rate_status);
                f.render_widget(Clear, area);
                f.render_widget(heart_rate_overlay, area);
            }

            // TODO Ask to save device?

            // // Draw the connecting overlay
            // if app.error_view {
            //     let connecting_device_info = app.error_message.clone();
            //     let area = centered_rect(60, 50, f.size());
            //     let error_block = Paragraph::new(Span::from(connecting_device_info))
            //         .alignment(Alignment::Center)
            //         .block(Block::default().borders(Borders::ALL).title("Notification"));
            //     f.render_widget(Clear, area);
            //     f.render_widget(error_block, area);
            // }

            // Draw the error overlay if the string is not empty
            if let Some(error_message_clone) = app.error_message.clone() {
                let area = centered_rect(60, 50, f.size());
                let error_block = Paragraph::new(Span::from(error_message_clone))
                    .alignment(Alignment::Center)
                    .block(Block::default().borders(Borders::ALL).title("Notification"));
                f.render_widget(Clear, area);
                f.render_widget(error_block, area);
            }
        })?;

        // Event handling
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let idle_on_main_menu = app.error_message.is_none()
                    && !app.inspect_view
                    && !app.is_loading_characteristics
                    && !app.heart_rate_display
                    && !app.is_connecting;

                match key.code {
                    KeyCode::Char('e') => {
                        app.error_message = Some("This is a test error message".to_string());
                    }
                    KeyCode::Char('q') => {
                        break;
                        // TODO Gracefully disconnect bluetooth?
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        if key.modifiers == KeyModifiers::CONTROL {
                            break;
                            // TODO Gracefully disconnect bluetooth?
                        } else {
                            if idle_on_main_menu {
                                app.is_loading_characteristics = true;
                                app.connect_for_characteristics().await;
                            }
                        }
                    }
                    KeyCode::Char('s') => {
                        if idle_on_main_menu {
                            let current_state = app.ble_scan_paused.load(Ordering::SeqCst);
                            app.ble_scan_paused.store(!current_state, Ordering::SeqCst);
                        }
                    }
                    KeyCode::Enter => {
                        if app.error_message.is_some() {
                            app.error_message = None;
                        } else if app.inspect_view {
                            app.inspect_view = false;
                        } else if idle_on_main_menu {
                            app.connect_for_hr().await;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if app.inspect_view {
                            app.inspect_overlay_scroll += 1;
                        } else if !app.discovered_devices.is_empty() {
                            let next = match app.table_state.selected() {
                                Some(selected) => {
                                    if selected >= app.discovered_devices.len() - 1 {
                                        0
                                    } else {
                                        selected + 1
                                    }
                                }
                                None => 0,
                            };
                            app.table_state.select(Some(next));
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if app.inspect_view {
                            app.inspect_overlay_scroll =
                                app.inspect_overlay_scroll.saturating_sub(1);
                        } else {
                            let previous = match app.table_state.selected() {
                                Some(selected) => {
                                    if selected == 0 {
                                        app.discovered_devices.len().checked_sub(1).unwrap_or(0)
                                    } else {
                                        selected - 1
                                    }
                                }
                                None => 0,
                            };
                            app.table_state.select(Some(previous));
                        }
                    }
                    _ => {}
                }
            }
        }

        // Check for updates from BLE Discovery
        if let Ok(new_device_info) = app.app_rx.try_recv() {
            match new_device_info {
                DeviceData::DeviceInfo(device) => {
                    if let Some(existing_device) = app
                        .discovered_devices
                        .iter_mut()
                        .find(|d| d.id == device.id)
                    {
                        *existing_device = device;
                    } else {
                        app.discovered_devices.push(device);
                    }
                }
                DeviceData::Characteristics(characteristics) => {
                    app.selected_characteristics = characteristics;
                    app.inspect_view = true;
                    app.is_loading_characteristics = false;
                }
                DeviceData::Error(error) => {
                    app.error_message = Some(error);
                    app.is_loading_characteristics = false;
                }
            }

            if app.table_state.selected().is_none() {
                app.table_state.select(Some(0));
            }
        }

        // HR Notification Updates
        if let Ok(hr_data) = app.hr_rx.try_recv() {
            match hr_data {
                HeartRateData::HeartRateStatus(hr) => {
                    //app.heart_rate_display = true;
                    app.heart_rate_status = hr;
                }
                HeartRateData::Error(error) => {
                    app.error_message = Some(error);
                    app.is_loading_characteristics = false;
                }
                HeartRateData::Connected => {
                    app.is_loading_characteristics = false;
                    app.heart_rate_display = true;
                }
                HeartRateData::Disconnected => {
                    app.is_loading_characteristics = false;
                    app.heart_rate_display = false;
                }
            }
        }
    }
    Ok(())
}
