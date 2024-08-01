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

pub fn render_combined_chart(
    f: &mut Frame,
    area: Rect,
    rr_reactive: bool,
    bpm_history: Option<&VecDeque<f64>>,
    rr_history: Option<&VecDeque<f64>>,
    bpm_session_high: &(f64, DateTime<Local>),
    bpm_session_low: &(f64, DateTime<Local>),
    rr_session_high: &f64,
    rr_session_low: &f64,
) {
    let mut datasets = Vec::new();
    let has_bpm = bpm_history.is_some();
    let has_rr = rr_history.is_some();
    let bpm_bounds = [bpm_session_low.0, bpm_session_high.0];
    let avg_bpm = ((bpm_session_low.0 + bpm_session_high.0) / 2.0).ceil();

    let rr_low: f64 = if rr_reactive && has_rr {
        *rr_history
            .unwrap()
            .iter()
            .reduce(|a, b| if a < b { a } else { b })
            .unwrap_or(&0.0)
    } else {
        *rr_session_low
    };

    let rr_high: f64 = if rr_reactive && has_rr {
        *rr_history
            .unwrap()
            .iter()
            .reduce(|a, b| if a > b { a } else { b })
            .unwrap_or(&0.0)
    } else {
        *rr_session_high
    };

    let rr_bounds = [rr_low, rr_high];
    let avg_rr = (rr_low + rr_high) / 2.0;

    let rr_data: Vec<(f64, f64)> = if has_rr {
        rr_history
            .unwrap()
            .iter()
            .rev()
            .enumerate()
            .map(|(i, &x)| {
                let normalized = (x - rr_bounds[0]) / (rr_bounds[1] - rr_bounds[0]);
                let scaled = normalized * (bpm_bounds[1] - bpm_bounds[0]) + bpm_bounds[0];
                (i as f64, scaled)
            })
            .collect()
    } else {
        Vec::new()
    };

    if has_rr {
        datasets.push(
            Dataset::default()
                .name("(RR)")
                .graph_type(GraphType::Line)
                .marker(symbols::Marker::Dot)
                .style(Style::default().fg(Color::Blue))
                .data(&rr_data),
        );
    }

    let bpm_data: Vec<(f64, f64)> = if has_bpm {
        bpm_history
            .unwrap()
            .iter()
            .rev()
            .enumerate()
            .map(|(i, &x)| (i as f64, x))
            .collect()
    } else {
        Vec::new()
    };

    if has_bpm {
        datasets.push(
            Dataset::default()
                .name("BPM")
                .graph_type(GraphType::Line)
                .marker(symbols::Marker::Dot)
                .style(Style::default().fg(Color::Red))
                .data(&bpm_data),
        );
    }

    let labels = if has_bpm && has_rr {
        vec![
            format!("{} ({:.1})", bpm_bounds[0], rr_bounds[0]).bold(),
            format!("{} ({:.1})", avg_bpm, avg_rr).bold().into(),
            format!("{} ({:.1})", bpm_bounds[1], rr_bounds[1]).bold(),
        ]
    } else if has_bpm {
        vec![
            format!("{}", bpm_bounds[0]).bold(),
            format!("{}", avg_bpm).bold().into(),
            format!("{}", bpm_bounds[1]).bold(),
        ]
    } else {
        vec![
            format!("{:.1}", rr_bounds[0]).bold(),
            format!("{:.1}", avg_rr).bold().into(),
            format!("{:.1}", rr_bounds[1]).bold(),
        ]
    };

    let chart = Chart::new(datasets)
        .block(Block::bordered().title("Histogram".cyan().bold()))
        .x_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, CHART_RR_MAX_ELEMENTS as f64]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .labels(labels)
                .bounds(bpm_bounds),
        );
    f.render_widget(chart, area);
}
