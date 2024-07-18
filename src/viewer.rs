use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::{
    event::DisableMouseCapture,
    execute,
    terminal::{disable_raw_mode, LeaveAlternateScreen},
};
use futures::SinkExt;
use human_panic::Metadata;
use ratatui::backend::Backend;
use ratatui::layout::Alignment;
use ratatui::style::Style;
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

use crate::app::{App, AppState, DeviceData};
use crate::heart_rate::{MonitorData, HEART_RATE_SERVICE_UUID};
use crate::structs::DeviceInfo;
use crate::utils::centered_rect;
use crate::widgets::detail_table::detail_table;
use crate::widgets::device_table::device_table;
use crate::widgets::heart_rate_display::heart_rate_display;
use crate::widgets::info_table::info_table;
use crate::widgets::inspect_overlay::inspect_overlay;
use crate::widgets::save_prompt::save_prompt;

// https://ratatui.rs/recipes/apps/better-panic/
pub fn initialize_panic_handler() -> Result<(), Box<dyn Error>> {
    let (panic_hook, eyre_hook) = color_eyre::config::HookBuilder::default()
        .panic_section(format!(
            "This is a bug. Consider reporting it at {}",
            //env!("CARGO_PKG_REPOSITORY")
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        ))
        .display_location_section(true)
        .display_env_section(true)
        .into_hooks();
    eyre_hook.install()?;
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);

        let msg = format!("{}", panic_hook.panic_report(panic_info));
        #[cfg(not(debug_assertions))]
        {
            eprintln!("{}", msg); // prints color-eyre stack trace to stderr
            use human_panic::{handle_dump, print_msg, Metadata};
            let meta = Metadata::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

            let file_path = handle_dump(&meta, panic_info);
            // prints human-panic message
            print_msg(file_path, &meta)
                .expect("human-panic: printing error message to console failed");
        }
        eprintln!("Error: {}", strip_ansi_escapes::strip_str(msg));

        #[cfg(debug_assertions)]
        {
            // Better Panic stacktrace that is only enabled when debugging.
            better_panic::Settings::auto()
                .most_recent_first(false)
                .lineno_suffix(true)
                .verbosity(better_panic::Verbosity::Full)
                .create_panic_handler()(panic_info);
        }

        std::process::exit(libc::EXIT_FAILURE);
    }));
    Ok(())
}

/// Displays the detected Bluetooth devices in a table and handles the user input.
/// The user can navigate the table, pause the scanning, and quit the application.
/// The detected devices are received through the provided `mpsc::Receiver`.
pub async fn viewer<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<(), Box<dyn Error>> {
    // Defining a custom panic hook to reset the terminal properties
    initialize_panic_handler()?;

    app.table_state.select(Some(0));
    app.save_prompt_state.select(Some(0));

    // Big loop here, drawing the different possible UIs
    // then handing all events (keys, bt, bt -> osc)

    // TODO Make this shit smaller
    loop {
        // Draw UI
        terminal.draw(|f| {
            app.frame_count = f.count();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .vertical_margin(1)
                .constraints(
                    [
                        Constraint::Percentage(70),
                        Constraint::Percentage(30),
                        Constraint::Min(1),
                    ]
                    .as_ref(),
                )
                .split(f.size());

            let device_binding = &DeviceInfo::default();
            // Causes a borrow issue, strange.
            // let selected_device = app.get_selected_device().unwrap_or(device_binding);
            let selected_device = app
                .discovered_devices
                .get(app.table_state.selected().unwrap_or(0))
                .unwrap_or(device_binding);

            match app.app_state {
                AppState::MainMenu
                | AppState::CharacteristicView
                | AppState::ConnectingForCharacteristics
                | AppState::ConnectingForHeartRate
                | AppState::SaveDevicePrompt => {
                    // Draw the device table
                    let device_table =
                        device_table(app.table_state.selected(), &app.discovered_devices);
                    f.render_stateful_widget(device_table, chunks[0], &mut app.table_state);

                    // Draw the detail table
                    let detail_table = detail_table(selected_device);
                    f.render_widget(detail_table, chunks[1]);

                    // Draw the info table
                    app.frame_count += 1;
                    let info_table: ratatui::widgets::Table<'_> = info_table(
                        app.ble_scan_paused.load(Ordering::SeqCst),
                        app.app_state == AppState::ConnectingForCharacteristics,
                        &app.frame_count,
                    );
                    f.render_widget(info_table, chunks[2]);

                    // Draw the inspect overlay
                    if app.app_state == AppState::CharacteristicView {
                        let area = centered_rect(60, 60, f.size());
                        let inspect_overlay = inspect_overlay(
                            &app.selected_characteristics,
                            app.characteristic_scroll,
                            area.height,
                        );
                        f.render_widget(Clear, area);
                        f.render_widget(inspect_overlay, area);
                    } else if app.app_state == AppState::SaveDevicePrompt {
                        let area = centered_rect(30, 30, f.size());
                        let save_device_prompt =
                            save_prompt(app.save_prompt_state.selected(), selected_device);
                        f.render_stateful_widget(
                            save_device_prompt,
                            area,
                            &mut app.save_prompt_state,
                        );
                    }
                }
                AppState::HeartRateViewNoData | AppState::HeartRateView => {
                    // Draw the heart rate overlay
                    let area = centered_rect(100, 100, f.size());
                    let heart_rate_overlay = heart_rate_display(&app.heart_rate_status);
                    //f.render_widget(Clear, area);
                    f.render_widget(heart_rate_overlay, area);
                }
            }

            if app.app_state == AppState::ConnectingForHeartRate
                || app.app_state == AppState::HeartRateViewNoData
            {
                let area = centered_rect(50, 50, f.size());
                let mut border_style = Style::default();

                let mut name = app
                    .get_selected_device()
                    .unwrap_or(selected_device)
                    .name
                    .clone();
                // Set border to green if we're quick-connecting.
                if app.quick_connect_ui {
                    border_style = Style::default().fg(ratatui::style::Color::Green);
                    if name == "Unknown" {
                        name = "Saved Device".into();
                    }
                }

                let connecting_block = Paragraph::new(format!(
                    "Connecting to:\n{}\n({})",
                    name,
                    selected_device.get_id()
                ))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(border_style),
                );
                f.render_widget(Clear, area);
                f.render_widget(connecting_block, area);
            }

            // Draw the error overlay if the string is not empty
            if let Some(error_message_clone) = app.error_message.clone() {
                let area = centered_rect(60, 50, f.size());
                let error_block = Paragraph::new(Span::from(error_message_clone))
                    .alignment(Alignment::Center)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("! Notification !"),
                    );
                f.render_widget(Clear, area);
                f.render_widget(error_block, area);
            }
        })?;

        // Event handling
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let idle_on_main_menu =
                    app.error_message.is_none() && app.app_state == AppState::MainMenu;

                match key.code {
                    KeyCode::Char('e') => {
                        app.error_message = Some("This is a test error message".to_string());
                    }
                    KeyCode::Char('q') => {
                        // if app.app_state == AppState::MainMenu {
                        break;
                        // }
                        // TODO Gracefully disconnect bluetooth?
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        if key.modifiers == KeyModifiers::CONTROL {
                            break;
                            // TODO Gracefully disconnect bluetooth?
                        } else {
                            if idle_on_main_menu {
                                app.app_state = AppState::ConnectingForCharacteristics;
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
                    // Enter!
                    KeyCode::Enter => {
                        if app.error_message.is_some() {
                            app.error_message = None;
                        } else if app.app_state == AppState::CharacteristicView {
                            app.app_state = AppState::MainMenu;
                        } else if app.app_state == AppState::SaveDevicePrompt {
                            let chosen_option = app.save_prompt_state.selected().unwrap_or(0);
                            // TODO Make this not weirdly hard-coded numbers?
                            match chosen_option {
                                0 => {
                                    app.allow_saving = true;
                                    app.save_settings().expect("Failed to save settings");
                                }
                                1 => {}
                                2 => {
                                    app.settings.ble.never_ask_to_save = true;
                                    app.save_settings().expect("Failed to save settings");
                                }
                                _ => {}
                            }
                            app.connect_for_hr(None).await;
                        } else if idle_on_main_menu {
                            // app_state changed by method
                            app.connect_for_hr(None).await;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        // Ugly, fix.
                        // TODO See if you can generalize this + Down, especially for the Save dialog
                        // Use match on the app_state?
                        if app.app_state == AppState::CharacteristicView {
                            app.characteristic_scroll += 1;
                        } else if app.app_state == AppState::MainMenu
                            && !app.discovered_devices.is_empty()
                        {
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
                        } else if app.app_state == AppState::SaveDevicePrompt {
                            let next = match app.save_prompt_state.selected() {
                                Some(selected) => {
                                    if selected >= 3 - 1 {
                                        0
                                    } else {
                                        selected + 1
                                    }
                                }
                                None => 0,
                            };
                            app.save_prompt_state.select(Some(next));
                        }
                    }
                    // Ditto.
                    KeyCode::Up | KeyCode::Char('k') => {
                        if app.app_state == AppState::CharacteristicView {
                            app.characteristic_scroll = app.characteristic_scroll.saturating_sub(1);
                        } else if app.app_state == AppState::MainMenu {
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
                        } else if app.app_state == AppState::SaveDevicePrompt {
                            let previous = match app.save_prompt_state.selected() {
                                Some(selected) => {
                                    if selected == 0 {
                                        3 - 1
                                    } else {
                                        selected - 1
                                    }
                                }
                                None => 0,
                            };
                            app.save_prompt_state.select(Some(previous));
                        }
                    }
                    _ => {}
                }
            }
        }

        // Check for updates from BLE Thread
        if let Ok(new_device_info) = app.ble_rx.try_recv() {
            let idle_on_main_menu =
                app.error_message.is_none() && app.app_state == AppState::MainMenu;
            match new_device_info {
                DeviceData::DeviceInfo(device) => {
                    // If the device is already in the list, update it
                    if let Some(existing_device_index) = app
                        .discovered_devices
                        .iter_mut()
                        .position(|d| d.id == device.id)
                    {
                        //*existing_device = device.clone();
                        app.discovered_devices[existing_device_index] = device.clone();
                    } else {
                        // If the device is not in the list, add it
                        // but only if it has the heart rate service
                        if device
                            .services
                            .iter()
                            .any(|service| service.clone() == HEART_RATE_SERVICE_UUID)
                        {
                            app.discovered_devices.push(device.clone());
                        }
                        // This filter used to be in scan.rs, but doing it here
                        // lets us connect to saved devices without checking their services (i.e. quicker)
                    }

                    // If the device is saved, connect to it
                    if device.id == app.settings.ble.saved_address
                        || device.name == app.settings.ble.saved_name && idle_on_main_menu
                    {
                        app.quick_connect_ui = true;
                        // I'm going to assume that if we find a set saved device,
                        // they're always going to want to update the value in case Name/MAC changes,
                        // even if they're weird and have set `never_ask_to_save` to true
                        app.allow_saving = true;
                        app.table_state.select(
                            app.discovered_devices
                                .iter()
                                .position(|d| d.id == device.id),
                        );
                        // app_state changed by method
                        app.connect_for_hr(Some(&device)).await;
                    }

                    if app.allow_saving && app.get_selected_device().unwrap().id == device.id {
                        let new_id = device.get_id();
                        let new_name = device.name.clone();
                        let mut damaged = false;
                        if app.settings.ble.saved_address != new_id {
                            app.settings.ble.saved_address = new_id;
                            damaged = true;
                        }
                        // TODO See if I can find a way to get "Unknown" programatically,
                        // not a fan of hardcoding it (and it's "" in the ::default())
                        // Maybe do a .new() and supply a None?
                        if app.settings.ble.saved_name != new_name && new_name != "Unknown" {
                            app.settings.ble.saved_name = new_name;
                            damaged = true;
                        }
                        if damaged {
                            app.save_settings().expect("Failed to save settings");
                        }
                    }
                }
                DeviceData::Characteristics(characteristics) => {
                    app.selected_characteristics = characteristics;
                    app.app_state = AppState::CharacteristicView
                }
                DeviceData::Error(error) => {
                    app.error_message = Some(error);
                    //app.is_loading_characteristics = false;
                }
            }

            if app.table_state.selected().is_none() {
                app.table_state.select(Some(0));
            }
        }

        // HR Notification Updates
        if let Ok(hr_data) = app.hr_rx.try_recv() {
            match hr_data {
                MonitorData::HeartRateStatus(hr) => {
                    //app.heart_rate_display = true;
                    app.heart_rate_status = hr;
                    // Assume we have proper data now
                    app.app_state = AppState::HeartRateView;
                }
                MonitorData::Error(error) => {
                    app.error_message = Some(error);
                    //app.is_loading_characteristics = false;
                }
                MonitorData::Connected => {
                    // TODO Maybe this should reset everything since it's a connect?
                    //app.is_loading_characteristics = false;
                    app.heart_rate_display = true;
                    app.app_state = if app.heart_rate_status.heart_rate_bpm > 0 {
                        AppState::HeartRateView
                    } else {
                        AppState::HeartRateViewNoData
                    };
                }
                MonitorData::Disconnected => {
                    //app.is_loading_characteristics = false;
                    //app.heart_rate_display = false;

                    // TODO Reconnect?
                    app.error_message = Some("Disconnected from device!".to_string());
                }
            }
            //app.hr_tx.send(hr_data.clone()).unwrap();
        }
    }
    Ok(())
}
