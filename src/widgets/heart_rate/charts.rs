use ratatui::{
    layout::Rect,
    style::{Color, Style, Stylize},
    symbols,
    widgets::{Axis, Block, Chart, Dataset, GraphType},
    Frame,
};

use crate::{
    app::App,
    widgets::heart_rate_display::{CHART_BPM_MAX_ELEMENTS, CHART_RR_MAX_ELEMENTS},
};

pub fn render_combined_chart(
    f: &mut Frame,
    area: Rect,
    app: &App,
    render_bpm: bool,
    render_rr: bool,
) {
    let mut datasets = Vec::new();

    let rr_bounds = [app.chart_low_rr, app.chart_high_rr];
    let avg_rr = app.chart_mid_rr;
    let bpm_bounds = [app.chart_low_bpm, app.chart_high_bpm];
    let avg_bpm = app.chart_mid_bpm;

    let combine_charts = render_bpm && render_rr;
    // By default we have the combined chart enabled, but if the user's
    // monitor doesn't support RR, we should hide just the RR portion
    let hide_rr = combine_charts && app.rr_dataset.is_empty();
    let render_rr = render_rr && !hide_rr;

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
            format!("{} ({:.1})", avg_bpm, avg_rr).bold(),
            format!("{} ({:.1})", bpm_bounds[1], rr_bounds[1]).bold(),
        ]
    } else if render_bpm {
        vec![
            format!("{}", bpm_bounds[0]).bold(),
            format!("{}", avg_bpm).bold(),
            format!("{}", bpm_bounds[1]).bold(),
        ]
    } else {
        vec![
            format!("{:.1}", rr_bounds[0]).bold(),
            format!("{:.1}", avg_rr).bold(),
            format!("{:.1}", rr_bounds[1]).bold(),
        ]
    };

    let y_bounds = if render_bpm { bpm_bounds } else { rr_bounds };

    let x_bound_top = if render_rr && render_bpm {
        CHART_BPM_MAX_ELEMENTS.max(CHART_RR_MAX_ELEMENTS)
    } else if render_bpm {
        CHART_BPM_MAX_ELEMENTS
    } else {
        CHART_RR_MAX_ELEMENTS
    };

    let chart = Chart::new(datasets)
        .block(Block::bordered().title("Histogram".cyan().bold()))
        .x_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, x_bound_top as f64]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(Color::Gray))
                .labels(labels)
                .bounds(y_bounds),
        );
    f.render_widget(chart, area);
}
