use std::collections::VecDeque;

use chrono::{DateTime, Local};
use ratatui::{
    layout::Rect,
    style::{Color, Style, Stylize},
    symbols,
    widgets::{Axis, Block, Chart, Dataset, GraphType},
    Frame,
};

use crate::{
    app::App,
    widgets::heart_rate_display::{
        CHART_BPM_MAX_ELEMENTS, CHART_BPM_VERT_MARGIN, CHART_RR_MAX_ELEMENTS, CHART_RR_VERT_MARGIN,
    },
};

pub fn render_combined_chart(
    f: &mut Frame,
    area: Rect,
    app: &App,
    render_bpm: bool,
    render_rr: bool,
) {
    let mut datasets = Vec::new();

    let rr_bounds = [app.session_low_rr, app.session_high_rr];
    let avg_rr = app.session_avg_rr;
    let bpm_bounds = [app.session_low_bpm.0, app.session_high_bpm.0];
    let avg_bpm = app.session_avg_bpm;

    if render_rr {
        datasets.push(
            Dataset::default()
                .name("(RR)")
                .graph_type(GraphType::Line)
                .marker(symbols::Marker::Dot)
                .style(Style::default().fg(Color::Blue))
                .data(&app.rr_dataset),
        );
    }

    if render_bpm {
        datasets.push(
            Dataset::default()
                .name("BPM")
                .graph_type(GraphType::Line)
                .marker(symbols::Marker::Dot)
                .style(Style::default().fg(Color::Red))
                .data(&app.bpm_dataset),
        );
    }

    let labels = if render_bpm && render_rr {
        vec![
            format!("{} ({:.1})", bpm_bounds[0], rr_bounds[0]).bold(),
            format!("{} ({:.1})", avg_bpm, avg_rr).bold().into(),
            format!("{} ({:.1})", bpm_bounds[1], rr_bounds[1]).bold(),
        ]
    } else if render_bpm {
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

    let bounds = if render_rr && render_bpm {
        bpm_bounds
    } else if render_bpm {
        bpm_bounds
    } else {
        rr_bounds
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
                .bounds(bounds),
        );
    f.render_widget(chart, area);
}
