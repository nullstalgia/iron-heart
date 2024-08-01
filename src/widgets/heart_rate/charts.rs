use std::collections::VecDeque;

use chrono::{DateTime, Local};
use ratatui::{
    layout::Rect,
    style::{Color, Style, Stylize},
    symbols,
    widgets::{Axis, Block, Chart, Dataset, GraphType},
    Frame,
};

use crate::widgets::heart_rate_display::{
    CHART_BPM_MAX_ELEMENTS, CHART_BPM_VERT_MARGIN, CHART_RR_MAX_ELEMENTS, CHART_RR_VERT_MARGIN,
};

pub fn render_bpm_chart(
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
        .graph_type(GraphType::Line)
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
pub fn render_rr_chart(
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
        .graph_type(GraphType::Line)
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

// TODO maybe combine renderings?
pub fn render_combined_chart(
    f: &mut Frame,
    area: Rect,
    rr_reactive: bool,
    bpm_data: &VecDeque<f64>,
    rr_data: &VecDeque<f64>,
    bpm_session_high: &(f64, DateTime<Local>),
    bpm_session_low: &(f64, DateTime<Local>),
    rr_session_high: &f64,
    rr_session_low: &f64,
) {
    let bpm_data: Vec<(f64, f64)> = bpm_data
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &x)| (i as f64, x))
        .collect();

    let bpm_bounds = [bpm_session_low.0, bpm_session_high.0];
    let avg_bpm = ((bpm_session_low.0 + bpm_session_high.0) / 2.0).ceil();

    let rr_low = if rr_reactive {
        rr_data
            .iter()
            .reduce(|a, b| if a < b { a } else { b })
            .unwrap_or(&0.0)
    } else {
        rr_session_low
    };
    let rr_high = if rr_reactive {
        rr_data
            .iter()
            .reduce(|a, b| if a > b { a } else { b })
            .unwrap_or(&0.0)
    } else {
        rr_session_high
    };
    let rr_bounds = [(rr_low), (rr_high)];
    let avg_rr = (rr_low + rr_high) / 2.0;

    let rr_data: Vec<(f64, f64)> = rr_data
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &x)| {
            let normalized = (x - rr_bounds[0]) / (rr_bounds[1] - rr_bounds[0]);
            let scaled = normalized * (bpm_bounds[1] - bpm_bounds[0]) + bpm_bounds[0];
            (i as f64, scaled)
        })
        .collect();
    let datasets = vec![
        Dataset::default()
            .name("(RR)")
            .graph_type(GraphType::Line)
            .marker(symbols::Marker::Dot)
            .style(Style::default().fg(Color::Blue))
            .data(&rr_data),
        Dataset::default()
            .name("BPM")
            .graph_type(GraphType::Line)
            .marker(symbols::Marker::Dot)
            .style(Style::default().fg(Color::Red))
            .data(&bpm_data),
    ];

    let chart = Chart::new(datasets)
        .block(Block::bordered().title("BPM History (and Beat-Beat Interval)".cyan().bold()))
        .x_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, CHART_RR_MAX_ELEMENTS as f64]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .labels(vec![
                    format!("{} ({:.1})", bpm_bounds[0], rr_bounds[0]).bold(),
                    format!("{} ({:.1})", avg_bpm, avg_rr).bold().into(),
                    format!("{} ({:.1})", bpm_bounds[1], rr_bounds[1]).bold(),
                ])
                .bounds(bpm_bounds),
        );
    f.render_widget(chart, area);
}
