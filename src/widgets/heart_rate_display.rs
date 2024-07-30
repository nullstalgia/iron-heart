use std::collections::VecDeque;

use chrono::{DateTime, Local};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols,
    text::Span,
    widgets::{Axis, Block, Borders, Chart, Dataset, Row, Table},
    Frame,
};

use crate::{
    app::App,
    heart_rate::{BatteryLevel, HeartRateStatus},
};

// TODO Ascii Heart Beat Animation

pub const CHART_BPM_MAX_ELEMENTS: usize = 60;
pub const CHART_RR_MAX_ELEMENTS: usize = 60;
const CHART_BPM_VERT_MARGIN: f64 = 5.0;

fn render_table(
    heart_rate_status: &HeartRateStatus,
    session_high: &(f64, DateTime<Local>),
    session_low: &(f64, DateTime<Local>),
    session_stats_use_12hr: bool,
) -> Table<'static> {
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

    let time_format = if session_stats_use_12hr {
        "%-I:%M %p"
    } else {
        "%H:%M"
    };

    rows.push(Row::new(vec![
        heart_rate_status.heart_rate_bpm.to_string(),
        format!(
            "{:.3?}",
            heart_rate_status
                .rr_intervals
                .iter()
                .map(|rr| rr.as_secs_f32())
                .collect::<Vec<f32>>()
        ),
        battery_string,
        format!(
            "{:.0} BPM @ {}",
            session_high.0,
            session_high.1.format(time_format)
        ),
        format!(
            "{:.0} BPM @ {}",
            session_low.0,
            session_low.1.format(time_format)
        ),
    ]));

    Table::new(
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
    .highlight_style(Style::default().add_modifier(Modifier::BOLD))
}

fn render_chart(
    f: &mut Frame,
    area: Rect,
    bpm_data: &VecDeque<f64>,
    session_high: &(f64, DateTime<Local>),
    session_low: &(f64, DateTime<Local>),
)
//-> Chart<'static>
{
    let x_labels = vec![
        Span::styled(
            //format!("{}", app.window[0]),
            "A",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(
            //format!("{}", (app.window[0] + app.window[1]) / 2.0)
            "B",
        ),
        Span::styled(
            // format!("{}", app.window[1]),
            "C",
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ];
    let index: f64 = 0.0;
    let bpm_data: Vec<(f64, f64)> = bpm_data
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &x)| (i as f64, x))
        .collect();
    let datasets = vec![
        Dataset::default()
            .name("BPM")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Red))
            .data(&bpm_data),
        // Dataset::default()
        //     .name("RR")
        //     .marker(symbols::Marker::Dot)
        //     .style(Style::default().fg(Color::Blue))
        //     .data(&rr_data),
    ];

    let bpm_bounds = [
        session_low.0 - CHART_BPM_VERT_MARGIN,
        session_high.0 + CHART_BPM_VERT_MARGIN,
    ];
    let avg_bpm = ((session_low.0 + session_high.0) / 2.0).ceil();

    let test = format!("!\n!\n!");

    let chart = Chart::new(datasets)
        .block(Block::bordered().title("Histogram".cyan().bold()))
        .x_axis(
            Axis::default()
                //.title("X Axis")
                .style(Style::default().fg(Color::Gray))
                //.labels(x_labels)
                .bounds([0.0, CHART_BPM_MAX_ELEMENTS as f64]),
        )
        .y_axis(
            Axis::default()
                //.title("Y Axis")
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

// TODO Option to combine charts

pub fn heart_rate_display(frame: &mut Frame, app: &App) {
    let area = frame.size();

    let vertical = Layout::vertical([Constraint::Min(4), Constraint::Percentage(100)]);
    let horizontal_shared = Layout::horizontal([Constraint::Percentage(100)]);
    let horizontal_split = Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]);
    let [status_area, bottom] = vertical.areas(area);
    let [bpm_history, rr_history] = horizontal_split.areas(bottom);
    let [shared_chart] = horizontal_shared.areas(bottom);

    frame.render_widget(
        render_table(
            &app.heart_rate_status,
            &app.session_high,
            &app.session_low,
            app.settings.misc.session_stats_use_12hr,
        ),
        status_area,
    );
    render_chart(
        frame,
        shared_chart,
        &app.heart_rate_history,
        &app.session_high,
        &app.session_low,
    );
}
