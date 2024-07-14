use ratatui::{
    layout::Constraint,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Row, Table},
};

use crate::{structs::DeviceInfo, utils::extract_manufacturer_data};

/// Creates a table with the detected BTLE devices.
pub fn device_table(selected: Option<usize>, devices: &[DeviceInfo]) -> Table {
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let rows: Vec<Row> = devices
        .iter()
        .enumerate()
        .map(|(i, device)| {
            let style = if selected == Some(i) {
                selected_style
            } else {
                Style::default()
            };
            Row::new(vec![
                device.name.clone(),
                device.get_id(),
                extract_manufacturer_data(&device.manufacturer_data).company_code,
                device.rssi.clone(),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(40),
            Constraint::Length(20),
            Constraint::Length(30),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec!["Name", "Identifier", "Manufacturer", "RSSI"])
            .style(Style::default().fg(Color::Yellow)),
    )
    .block(
        Block::default()
            .title("Detected Devices")
            .borders(Borders::ALL),
    )
    .highlight_style(selected_style);

    table
}
