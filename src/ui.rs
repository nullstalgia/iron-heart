use ratatui::{
    layout::Alignment,
    widgets::{Block, Borders, Clear, Paragraph, TableState},
    Frame,
};

use crate::{
    activities::tui::{render_activity_name_entry, render_activity_selection},
    app::{App, AppView, SubState},
    widgets::prompts::{connecting_popup, render_error_popup},
};

#[cfg(windows)]
use crate::vrcx::tui::vrcx_prompt;

use crate::structs::DeviceInfo;
use crate::utils::centered_rect;
use crate::widgets::action_bar::action_bar;
use crate::widgets::detail_table::detail_table;
use crate::widgets::device_table::device_table;
use crate::widgets::heart_rate_display::heart_rate_display;
use crate::widgets::inspect_overlay::inspect_overlay;
use crate::widgets::prompts::save_prompt;

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

    match app.view {
        AppView::BleDeviceSelection => {
            // Draw the device table
            let device_table = device_table(app.table_state.selected(), &app.discovered_devices);
            f.render_stateful_widget(device_table, chunks[0], &mut app.table_state);

            // Draw the detail table
            let detail_table = detail_table(selected_device);
            f.render_widget(detail_table, chunks[1]);

            // Draw the info table
            let info_table: ratatui::widgets::Table<'_> = action_bar(
                app.ble_scan_paused.load(Ordering::SeqCst),
                app.sub_state == SubState::ConnectingForCharacteristics,
                &app.frame_count,
            );
            f.render_widget(info_table, chunks[2]);
        }
        AppView::HeartRateView => {
            heart_rate_display(app, f);
        }
        AppView::WaitingForWebsocket => {
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

    // Overlays/substates
    match app.sub_state {
        SubState::CharacteristicView => {
            let area = centered_rect(60, 60, f.area());
            let inspect_overlay = inspect_overlay(
                &app.selected_characteristics,
                app.characteristic_scroll,
                area.height,
            );
            f.render_widget(Clear, area);
            f.render_widget(inspect_overlay, area);
        }
        SubState::SaveDevicePrompt => {
            let area = centered_rect(30, 30, f.area());
            let save_device_prompt = save_prompt();
            f.render_stateful_widget(save_device_prompt, area, &mut app.prompt_state);
        }
        SubState::ConnectingForHeartRate => {
            let area = centered_rect(50, 50, f.area());
            let connecting_block = connecting_popup(
                &selected_device.name,
                &selected_device.get_id(),
                app.quick_connect_ui,
            );
            f.render_widget(Clear, area);
            f.render_widget(connecting_block, area);
        }
        #[cfg(windows)]
        SubState::VrcxAutostartPrompt => {
            vrcx_prompt(app, f);
        }
        SubState::ActivitySelection => {
            render_activity_selection(app, f);
        }
        SubState::ActivityCreation => {
            render_activity_selection(app, f);
            render_activity_name_entry(app, f);
        }
        SubState::None | SubState::ConnectingForCharacteristics => {}
    }

    // Draw the error overlay if the string is not empty
    render_error_popup(app, f);
}

pub fn table_state_scroll(up: bool, state: &mut TableState, table_len: usize) {
    if table_len == 0 {
        return;
    }
    let next = match state.selected() {
        Some(selected) => {
            if up {
                (selected + table_len - 1) % table_len
            } else {
                (selected + 1) % table_len
            }
        }
        None => 0,
    };
    state.select(Some(next));
}
