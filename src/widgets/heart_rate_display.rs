use std::collections::HashMap;

use ratatui::{
    layout::Constraint,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Row, Table},
};

use crate::heart_rate::{BatteryLevel, HeartRateStatus};

// TODO Ascii Heart Beat Animation

/// Render just the heart rate, RR, and battery level.
pub fn heart_rate_display(heart_rate_status: &HeartRateStatus) -> Table<'static> {
    let mut rows: Vec<Row> = Vec::new();

    rows.push(
        Row::new(vec!["Heart Rate", "RR", "Battery Level"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    );

    let battery_string: String = match heart_rate_status.battery_level {
        BatteryLevel::Unknown => "???".into(),
        BatteryLevel::NotReported => "N/A".into(),
        BatteryLevel::Level(level) => level.to_string(),
    };

    rows.push(Row::new(vec![
        heart_rate_status.heart_rate_bpm.to_string(),
        format!("{:?}", heart_rate_status.rr_intervals),
        battery_string,
    ]));

    Table::new(
        rows.to_vec(),
        [
            Constraint::Length(20),
            Constraint::Length(20),
            Constraint::Length(20),
            Constraint::Length(10),
        ],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Heart Rate Monitor")
            .border_style(Style::default().fg(Color::Yellow)),
    )
    .highlight_style(Style::default().add_modifier(Modifier::BOLD))
}
