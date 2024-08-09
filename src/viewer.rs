use crossterm::event::{self, Event, KeyCode, KeyModifiers};

use log::*;
use ratatui::backend::Backend;
use ratatui::layout::Alignment;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Terminal,
};
use std::error::Error;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::app::{App, AppState, DeviceData, ErrorPopup};
use crate::heart_rate::{HeartRateStatus, HEART_RATE_SERVICE_UUID};
use crate::panic_handler::initialize_panic_handler;
use crate::structs::DeviceInfo;
use crate::utils::centered_rect;
use crate::widgets::detail_table::detail_table;
use crate::widgets::device_table::device_table;
use crate::widgets::heart_rate_display::heart_rate_display;
use crate::widgets::info_table::info_table;
use crate::widgets::inspect_overlay::inspect_overlay;
use crate::widgets::save_prompt::save_prompt;

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
    // then handing all events (keys, bt, bt -> osc | log | ui)

    // TODO Make this shit smaller
    loop {
        // In case another task called for a shutdown
        if app.shutdown_requested.is_cancelled() {
            warn!("Viewer recieved shutdown signal!");
            break;
        }

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
                .split(f.area());

            let device_binding = &DeviceInfo::default();
            // Causes a borrow issue, strange.
            // I think it's due to the .get() in .get_selected_device()
            // let selected_device = app.get_selected_device().unwrap_or(device_binding);
            let selected_device = app
                .discovered_devices
                .get(app.table_state.selected().unwrap_or(0))
                .unwrap_or(device_binding);

            match app.state {
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
                        app.state == AppState::ConnectingForCharacteristics,
                        &app.frame_count,
                    );
                    f.render_widget(info_table, chunks[2]);

                    // Draw the inspect overlay
                    if app.state == AppState::CharacteristicView {
                        let area = centered_rect(60, 60, f.area());
                        let inspect_overlay = inspect_overlay(
                            &app.selected_characteristics,
                            app.characteristic_scroll,
                            area.height,
                        );
                        f.render_widget(Clear, area);
                        f.render_widget(inspect_overlay, area);
                    } else if app.state == AppState::SaveDevicePrompt {
                        let area = centered_rect(30, 30, f.area());
                        let save_device_prompt = save_prompt();
                        f.render_stateful_widget(
                            save_device_prompt,
                            area,
                            &mut app.save_prompt_state,
                        );
                    }
                }
                AppState::HeartRateViewNoData | AppState::HeartRateView => {
                    heart_rate_display(f, app);
                }
                AppState::WaitingForWebsocket => {
                    // TODO Move out to a function
                    let area = centered_rect(60, 60, f.area());
                    let mut text = "Waiting for websocket connection...".to_string();
                    if let Some(ref url) = app.websocket_url {
                        let connection_info = if url.starts_with("0.0.0.0") {
                            local_ip_address::local_ip()
                                .map(|local_ip| format!("{}{}", local_ip, &url[7..]))
                                .unwrap_or_else(|_| url.clone())
                        } else {
                            url.clone()
                        };
                        text.push_str(&format!("\nConnect to: {}", connection_info));
                    }
                    let connecting_block = Paragraph::new(text)
                        .alignment(Alignment::Center)
                        .block(Block::default().borders(Borders::ALL));
                    f.render_widget(Clear, area);
                    f.render_widget(connecting_block, area);
                }
            }

            if app.state == AppState::ConnectingForHeartRate
                || app.state == AppState::HeartRateViewNoData
            {
                let area = centered_rect(50, 50, f.area());
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
                let (style, message) = match error_message_clone.clone() {
                    ErrorPopup::Fatal(msg) => {
                        (Style::default().fg(ratatui::style::Color::Red), msg)
                    }
                    ErrorPopup::Intermittent(msg) => {
                        (Style::default().fg(ratatui::style::Color::Yellow), msg)
                    }
                    ErrorPopup::UserMustDismiss(msg) => {
                        (Style::default().fg(ratatui::style::Color::Blue), msg)
                    }
                };

                let area = centered_rect(60, 50, f.area());
                let error_block = Paragraph::new(Span::from(message))
                    .alignment(Alignment::Center)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("! Notification !")
                            .style(style),
                    )
                    .wrap(Wrap { trim: true });
                f.render_widget(Clear, area);
                f.render_widget(error_block, area);
            }
        })?;

        // Event handling
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('e') => {
                        app.error_message = Some(ErrorPopup::UserMustDismiss(
                            "This is a test error message".to_string(),
                        ));
                        error!("This is a test error message");
                    }
                    KeyCode::Char('q') => {
                        if app.is_idle_on_main_menu() {
                            break;
                        }
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        if key.modifiers == KeyModifiers::CONTROL {
                            break;
                        } else if app.is_idle_on_main_menu() {
                            app.connect_for_characteristics().await;
                        }
                    }
                    KeyCode::Char('s') => {
                        if app.is_idle_on_main_menu() {
                            let current_state = app.ble_scan_paused.load(Ordering::SeqCst);
                            app.ble_scan_paused.store(!current_state, Ordering::SeqCst);
                            debug!("(S) Pausing BLE scan");
                        }
                    }
                    // Enter!
                    KeyCode::Enter => {
                        if app.error_message.is_some() {
                            match app.error_message.as_ref().unwrap() {
                                ErrorPopup::UserMustDismiss(_) | ErrorPopup::Intermittent(_) => {
                                    app.error_message = None;
                                }
                                ErrorPopup::Fatal(_) => {
                                    break;
                                }
                            }
                        } else if app.state == AppState::CharacteristicView {
                            app.state = AppState::MainMenu;
                        } else if app.state == AppState::SaveDevicePrompt {
                            let chosen_option = app.save_prompt_state.selected().unwrap_or(0);
                            const ALLOW_SAVING_OPTION: usize = 0;
                            const NO_ACTION_OPTION: usize = 1;
                            const NEVER_ASK_TO_SAVE_OPTION: usize = 2;

                            match chosen_option {
                                ALLOW_SAVING_OPTION => {
                                    app.allow_saving = true;
                                    app.save_settings().expect("Failed to save settings");
                                }
                                NO_ACTION_OPTION => {}
                                NEVER_ASK_TO_SAVE_OPTION => {
                                    app.settings.ble.never_ask_to_save = true;
                                    app.save_settings().expect("Failed to save settings");
                                }
                                _ => {}
                            }
                            debug!(
                                "Connecting from save prompt | Chosen option: {}",
                                chosen_option
                            );
                            app.connect_for_hr(None).await;
                        } else if app.is_idle_on_main_menu() {
                            // app_state changed by method
                            debug!("Connecting from main menu");
                            app.connect_for_hr(None).await;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        app.scroll_down();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        app.scroll_up();
                    }
                    _ => {}
                }
            }
        }

        // Check for updates from BLE Thread
        if let Ok(new_device_info) = app.ble_rx.try_recv() {
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
                        // (We don't use the ScanFilter from btleplug to allow quicker connection to saved devices,
                        // and since it reports only "Unknown" names for some reason)
                        if device
                            .services
                            .iter()
                            .any(|service| *service == HEART_RATE_SERVICE_UUID)
                        {
                            app.discovered_devices.push(device.clone());
                        }
                        // This filter used to be in scan.rs, but doing it here
                        // lets us connect to saved devices without checking their services (i.e. quicker)
                    }

                    // If the device is saved, connect to it
                    if (device.id == app.settings.ble.saved_address
                        || device.name == app.settings.ble.saved_name)
                        && app.is_idle_on_main_menu()
                    {
                        app.quick_connect_ui = true;
                        // I'm going to assume that if we find a set saved device,
                        // they're always going to want to update the value in case Name/MAC changes,
                        // even if they're weird and have set `never_ask_to_save` to true
                        app.allow_saving = true;
                        // Adding device to UI list so other parts of the app that check the selected device
                        // get the expected result
                        if !app.discovered_devices.iter().any(|d| d.id == device.id) {
                            app.discovered_devices.push(device.clone());
                        }
                        app.table_state.select(
                            app.discovered_devices
                                .iter()
                                .position(|d| d.id == device.id),
                        );
                        app.try_save_device(Some(&device));
                        debug!("Connecting to saved device, AppState: {:?}", app.state);
                        // app_state changed by method
                        app.connect_for_hr(Some(&device)).await;
                    } else {
                        app.try_save_device(None);
                    }
                }
                DeviceData::Characteristics(characteristics) => {
                    app.selected_characteristics = characteristics;
                    app.state = AppState::CharacteristicView
                }
                DeviceData::Error(error) => {
                    error!("BLE Thread Error: {:?}", error.clone());
                    if app.state == AppState::HeartRateViewNoData
                        && matches!(error, ErrorPopup::Intermittent(_))
                    {
                        // Ignoring the intermittent ones when we're in the inbetween state
                    } else {
                        // Don't override a fatal error
                        if !matches!(app.error_message, Some(ErrorPopup::Fatal(_))) {
                            app.error_message = Some(error);
                        }
                    }
                    if app.state == AppState::HeartRateView
                        || app.state == AppState::HeartRateViewNoData
                        || app.state == AppState::ConnectingForHeartRate
                    {
                        app.osc_tx.send(HeartRateStatus::default()).unwrap();
                    }
                    //app.is_loading_characteristics = false;
                }
                DeviceData::ConnectedEvent(id) => {
                    if app.state == AppState::ConnectingForCharacteristics {
                        app.state = AppState::CharacteristicView;
                    } else {
                        app.state = if app.heart_rate_status.heart_rate_bpm > 0 {
                            AppState::HeartRateView
                        } else {
                            AppState::HeartRateViewNoData
                        };
                    }

                    if app.state == AppState::HeartRateView
                        || app.state == AppState::HeartRateViewNoData
                        || app.state == AppState::ConnectingForHeartRate
                    {
                        if id == app.get_selected_device().unwrap().id {
                            debug!("Connected to device {:?}, stopping BLE scan", id);
                            app.ble_scan_paused.store(true, Ordering::SeqCst);
                        }
                        app.try_save_device(None);
                    }
                }
                DeviceData::DisconnectedEvent(id) => {
                    app.error_message = Some(ErrorPopup::Intermittent(
                        "Disconnected from device!".to_string(),
                    ));
                    if (app.state == AppState::HeartRateView
                        || app.state == AppState::HeartRateViewNoData
                        || app.state == AppState::MainMenu)
                        && id == app.get_selected_device().unwrap().id
                    {
                        debug!("Disconnected from device {:?}, resuming BLE scan", id);
                        app.osc_tx.send(HeartRateStatus::default()).unwrap();
                        app.ble_scan_paused.store(false, Ordering::SeqCst);
                    }
                }
                // Not possible to receive these here
                DeviceData::HeartRateStatus(_) | DeviceData::WebsocketReady(_) => {}
            }

            if app.table_state.selected().is_none() {
                app.table_state.select(Some(0));
            }
        }

        // HR Notification Updates
        // Impromptu HR Data Dispatcher
        if let Ok(hr_data) = app.hr_rx.try_recv() {
            match hr_data {
                DeviceData::HeartRateStatus(hr) => {
                    app.heart_rate_status = hr.clone();
                    // Assume we have proper data now
                    app.state = AppState::HeartRateView;
                    // Dismiss intermittent errors if we just got a notification packet
                    if matches!(app.error_message, Some(ErrorPopup::Intermittent(_))) {
                        app.error_message = None;
                    }
                    app.append_to_history(&hr);
                    app.osc_tx.send(hr.clone()).unwrap();
                    app.log_tx.send(hr).unwrap();
                }
                DeviceData::Error(error) => {
                    // Don't override a fatal error
                    if !matches!(app.error_message, Some(ErrorPopup::Fatal(_))) {
                        app.error_message = Some(error);
                    }
                    app.osc_tx.send(HeartRateStatus::default()).unwrap();
                }
                DeviceData::WebsocketReady(local_addr) => {
                    app.websocket_url = Some(local_addr.to_string());
                }
                _ => {}
            }
        }
    }
    Ok(())
}
