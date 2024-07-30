use std::collections::VecDeque;

use chrono::{DateTime, Local};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols,
    text::Span,
    widgets::{Axis, Block, Borders, Cell, Chart, Dataset, Row, Table},
    Frame,
};

use crate::{
    app::App,
    heart_rate::{BatteryLevel, HeartRateStatus},
};

// TODO Ascii Heart Beat Animation

pub const CHART_BPM_MAX_ELEMENTS: usize = 120;
pub const CHART_RR_MAX_ELEMENTS: usize = 120;
pub const CHART_BPM_VERT_MARGIN: f64 = 3.0;
pub const CHART_RR_VERT_MARGIN: f64 = 0.0;

fn render_table(
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
            Constraint::Length(25),
            Constraint::Length(20),
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

// TODO Option to combine charts
fn render_bpm_chart(
    f: &mut Frame,
    area: Rect,
    bpm_data: &VecDeque<f64>,
    session_high: &(f64, DateTime<Local>),
    session_low: &(f64, DateTime<Local>),
) {
    let bpm_data: Vec<(f64, f64)> = bpm_data
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &x)| (i as f64, x))
        .collect();
    let datasets = vec![Dataset::default()
        .name("BPM")
        .marker(symbols::Marker::Dot)
        .style(Style::default().fg(Color::Red))
        .data(&bpm_data)];

    let bpm_bounds = [session_low.0, session_high.0];
    let avg_bpm = ((session_low.0 + session_high.0) / 2.0).ceil();

    let chart = Chart::new(datasets)
        .block(Block::bordered().title("BPM History".cyan().bold()))
        .x_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, CHART_BPM_MAX_ELEMENTS as f64]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .labels(vec![
                    bpm_bounds[0].to_string().bold(),
                    avg_bpm.to_string().bold().into(),
                    bpm_bounds[1].to_string().bold(),
                ])
                .bounds(bpm_bounds),
        );

    f.render_widget(chart, area);
}
fn render_rr_chart(
    f: &mut Frame,
    area: Rect,
    rr_data: &VecDeque<f64>,
    session_high: &f64,
    session_low: &f64,
) {
    let rr_data: Vec<(f64, f64)> = rr_data
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &x)| (i as f64, x))
        .collect();
    let datasets = vec![Dataset::default()
        .name("RR")
        .marker(symbols::Marker::Dot)
        .style(Style::default().fg(Color::Blue))
        .data(&rr_data)];

    let rr_bounds = [(session_low).floor(), (session_high).ceil()];
    let avg_rr = format!("{:.1}", ((session_low + session_high) / 2.0));

    let chart = Chart::new(datasets)
        .block(Block::bordered().title("Beat-Beat Interval".cyan().bold()))
        .x_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, CHART_RR_MAX_ELEMENTS as f64]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .labels(vec![
                    rr_bounds[0].to_string().bold(),
                    avg_rr.bold().into(),
                    rr_bounds[1].to_string().bold(),
                ])
                .bounds(rr_bounds),
        );

    f.render_widget(chart, area);
}

pub fn heart_rate_display(frame: &mut Frame, app: &App) {
    let area = frame.size();

    let vertical = Layout::vertical([Constraint::Min(4), Constraint::Percentage(100)]);
    let horizontal_shared = Layout::horizontal([Constraint::Percentage(100)]);
    let horizontal_split =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]);
    let [status_area, bottom] = vertical.areas(area);
    let [bpm_history, rr_history] = horizontal_split.areas(bottom);
    let [shared_chart] = horizontal_shared.areas(bottom);

    render_table(
        frame,
        status_area,
        &app.heart_rate_status,
        &app.session_high_bpm,
        &app.session_low_bpm,
        app.settings.misc.session_stats_use_12hr,
    );
    render_bpm_chart(
        frame,
        bpm_history,
        &app.heart_rate_history,
        &app.session_high_bpm,
        &app.session_low_bpm,
    );
    render_rr_chart(
        frame,
        rr_history,
        &app.rr_history,
        &app.session_high_rr,
        &app.session_low_rr,
    );
}
