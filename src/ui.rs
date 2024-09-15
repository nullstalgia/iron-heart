use ratatui::{
    layout::Alignment,
    style::{Color, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, AppState, DeviceUpdate, ErrorPopup};

use crate::structs::DeviceInfo;
use crate::utils::centered_rect;
use crate::widgets::detail_table::detail_table;
use crate::widgets::device_table::device_table;
use crate::widgets::heart_rate_display::heart_rate_display;
use crate::widgets::info_table::info_table;
use crate::widgets::inspect_overlay::inspect_overlay;
use crate::widgets::save_prompt::save_prompt;
use ratatui::text::Span;

use ratatui::{
    layout::{Constraint, Direction, Layout},
    Terminal,
};

use std::sync::atomic::Ordering;

/// Renders the user interface widgets.
pub fn render(app: &mut App, f: &mut Frame) {
    //app.frame_count = f.count();
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
            let device_table = device_table(app.table_state.selected(), &app.discovered_devices);
            f.render_stateful_widget(device_table, chunks[0], &mut app.table_state);

            // Draw the detail table
            let detail_table = detail_table(selected_device);
            f.render_widget(detail_table, chunks[1]);

            // Draw the info table
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
                f.render_stateful_widget(save_device_prompt, area, &mut app.save_prompt_state);
            }
        }
        AppState::HeartRateViewNoData | AppState::HeartRateView => {
            heart_rate_display(app, f);
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

    if app.state == AppState::ConnectingForHeartRate || app.state == AppState::HeartRateViewNoData {
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
            ErrorPopup::Fatal(msg) => (Style::default().fg(ratatui::style::Color::Red), msg),
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
}
