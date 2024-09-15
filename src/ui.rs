use ratatui::{
    layout::Alignment,
    style::Style,
    text::Text,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, AppState, ErrorPopup};

use crate::structs::DeviceInfo;
use crate::utils::centered_rect;
use crate::widgets::detail_table::detail_table;
use crate::widgets::device_table::device_table;
use crate::widgets::heart_rate_display::heart_rate_display;
use crate::widgets::info_table::info_table;
use crate::widgets::inspect_overlay::inspect_overlay;
use crate::widgets::save_prompt::save_prompt;

use ratatui::layout::{Constraint, Direction, Layout};

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
        let (style, message, error_message, title) = match error_message_clone.clone() {
            ErrorPopup::FatalDetailed(msg, error) => (
                Style::default().fg(ratatui::style::Color::Red),
                msg,
                Some(error),
                "!! Error !!",
            ),
            ErrorPopup::Fatal(msg) => (
                Style::default().fg(ratatui::style::Color::Red),
                msg,
                None,
                "!! Error !!",
            ),
            ErrorPopup::Intermittent(msg) => (
                Style::default().fg(ratatui::style::Color::Yellow),
                msg,
                None,
                "Warning",
            ),
            ErrorPopup::UserMustDismiss(msg) => (
                Style::default().fg(ratatui::style::Color::Blue),
                msg,
                None,
                "!! Notification !!",
            ),
        };

        let area = centered_rect(60, 50, f.area());

        // Create the outer block with borders and title
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(style);

        // Draw the block
        f.render_widget(Clear, area);
        f.render_widget(&block, area);

        // Get the inner area of the block
        let inner_area = block.inner(area);

        // Check for special character and split message
        if let Some(error_message) = error_message {
            // Split the inner_area vertically into two
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Fill(1), Constraint::Fill(3)].as_ref())
                .split(inner_area);

            // Create the centered paragraph with first_part
            let first_paragraph = Paragraph::new(Text::from(message))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true });

            // Create the left-aligned paragraph with second_part
            let second_paragraph = Paragraph::new(Text::from(error_message))
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: false });

            // Render the paragraphs
            f.render_widget(first_paragraph, chunks[0]);
            f.render_widget(second_paragraph, chunks[1]);
        } else {
            // No special character found, proceed as before
            let error_paragraph = Paragraph::new(Text::from(message))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true });

            f.render_widget(error_paragraph, inner_area);
        }
    }
}
