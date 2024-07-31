use chrono::{DateTime, Local};
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table},
    Frame,
};

use crate::heart_rate::{BatteryLevel, HeartRateStatus};

pub fn render_table(
    f: &mut Frame,
    area: Rect,
    heart_rate_status: &HeartRateStatus,
    session_high: &(f64, DateTime<Local>),
    session_low: &(f64, DateTime<Local>),
    session_stats_use_12hr: bool,
) {
    let mut rows: Vec<Row> = Vec::new();

    rows.push(
        Row::new(vec![
            "Heart Rate",
            "RR (sec)",
            "Battery Level",
            "Session High",
            "Session Low",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD)),
    );
    let battery_string: String = match heart_rate_status.battery_level {
        BatteryLevel::Unknown => "???".into(),
        BatteryLevel::NotReported => "N/A".into(),
        BatteryLevel::Level(level) => format!("{}%", level),
    };

    let battery_style = match heart_rate_status.battery_level {
        BatteryLevel::Unknown => Style::default().fg(Color::Red),
        BatteryLevel::NotReported => Style::default().fg(Color::Yellow),
        BatteryLevel::Level(level) => Style::default().fg(match level {
            0..=29 => Color::Red,
            30..=59 => Color::Yellow,
            60..=79 => Color::LightGreen,
            _ => Color::Green,
        }),
    };

    let time_format = if session_stats_use_12hr {
        "%-I:%M %p"
    } else {
        "%H:%M"
    };

    let rr_string = format!(
        "{:.3?}",
        heart_rate_status
            .rr_intervals
            .iter()
            .map(|rr| rr.as_secs_f32())
            .collect::<Vec<f32>>()
    );

    let high_string = format!(
        "{:.0} BPM @ {}",
        session_high.0,
        session_high.1.format(time_format)
    );

    let low_string = format!(
        "{:.0} BPM @ {}",
        session_low.0,
        session_low.1.format(time_format)
    );

    rows.push(Row::new(vec![
        Cell::from(heart_rate_status.heart_rate_bpm.to_string()),
        Cell::from(rr_string),
        Cell::from(battery_string).style(battery_style),
        Cell::from(high_string),
        Cell::from(low_string),
    ]));

    let table = Table::new(
        rows.to_vec(),
        [
            Constraint::Length(15),
            Constraint::Length(20),
            Constraint::Length(15),
            Constraint::Length(20),
            Constraint::Length(20),
        ],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Most Recent Data")
            .border_style(Style::default().fg(Color::Yellow)),
    )
    .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(table, area);
}
