use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    symbols,
    text::Line,
    widgets::{Axis, Block, Borders, Chart, Clear, Dataset, GraphType, Paragraph, Wrap},
    Frame,
};

use ratatui_macros::{line, span};

use crate::{
    app::App,
    widgets::heart_rate_display::{CHART_BPM_MAX_ELEMENTS, CHART_RR_MAX_ELEMENTS},
};

pub enum ChartType {
    BPM,
    RR,
    Combined,
}

fn legend_rect(width: u16, height: u16, graph_area: Rect) -> Rect {
    let popup_size = Rect {
        width: width + 2,
        height: height + 2,
        ..Rect::default()
    };
    Rect {
        x: (graph_area.x + (graph_area.width - popup_size.width)).saturating_sub(1),
        y: graph_area.y + 1,
        ..popup_size
    }
}

fn bpm_rr_legend(chart_type: &ChartType, graph_area: Rect) -> (Paragraph, Rect) {
    let text = match chart_type {
        ChartType::Combined => {
            vec![
                line![span!(Color::Red; "BPM")],
                line![span!(Color::Blue; "(RR)")],
            ]
        }
        ChartType::BPM => {
            vec![line![span!(Color::Red; "BPM")]]
        }
        ChartType::RR => {
            vec![line![span!(Color::Blue; "(RR)")]]
        }
    };
    let max_line_length = text
        .iter()
        .map(|line| line.width())
        .max()
        .unwrap_or_default();

    let legend_area = legend_rect(
        max_line_length as u16,
        text.iter().count() as u16,
        graph_area,
    );

    let legend_block = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    //f.render_widget(Clear, area);
    (legend_block, legend_area)
}

fn styled_label(bpm: f64, rr: f64, chart_type: &ChartType, allow_space: bool) -> Line {
    let bpm_label_style = (Color::LightRed, Modifier::BOLD);
    let rr_label_style = (Color::LightBlue, Modifier::BOLD);
    let rr = format!("({:.1})", rr);
    // Not a fan of this, need to ask Ratatui peeps
    let spaces = if allow_space && bpm <= 99.0 {
        "  "
    } else {
        " "
    };

    match chart_type {
        ChartType::Combined => {
            line![
                span!(bpm_label_style; bpm),
                spaces,
                span!(rr_label_style; rr),
            ]
        }
        ChartType::BPM => {
            line![span!(bpm_label_style; bpm)]
        }
        ChartType::RR => {
            line![span!(rr_label_style; rr)]
        }
    }
}

pub fn render_combined_chart(f: &mut Frame, area: Rect, app: &App, chart_type: ChartType) {
    let mut datasets = Vec::new();

    let rr_bounds = [app.chart_low_rr, app.chart_high_rr];
    let mid_rr = app.chart_mid_rr;
    let bpm_bounds = [app.chart_low_bpm, app.chart_high_bpm];
    let mid_bpm = app.chart_mid_bpm;

    if matches!(chart_type, ChartType::Combined) || matches!(chart_type, ChartType::RR) {
        datasets.push(
            Dataset::default()
                .name("(RR)")
                .graph_type(GraphType::Line)
                .marker(symbols::Marker::Dot)
                .style(Style::default().fg(Color::Blue))
                .data(&app.rr_dataset),
        );
    }

    if matches!(chart_type, ChartType::Combined) || matches!(chart_type, ChartType::BPM) {
        datasets.push(
            Dataset::default()
                .name("BPM")
                .graph_type(GraphType::Line)
                .marker(symbols::Marker::Dot)
                .style(Style::default().fg(Color::Red))
                .data(&app.bpm_dataset),
        );
    }

    let allow_space = bpm_bounds[0] <= 99.0 && bpm_bounds[1] >= 100.0;

    let labels = vec![
        styled_label(bpm_bounds[0], rr_bounds[0], &chart_type, allow_space),
        styled_label(mid_bpm, mid_rr, &chart_type, allow_space),
        styled_label(bpm_bounds[1], rr_bounds[1], &chart_type, allow_space),
    ];

    let y_bounds = match chart_type {
        ChartType::Combined | ChartType::BPM => bpm_bounds,
        ChartType::RR => rr_bounds,
    };

    let x_bound_top = match chart_type {
        ChartType::Combined => CHART_BPM_MAX_ELEMENTS.max(CHART_RR_MAX_ELEMENTS),
        ChartType::BPM => CHART_BPM_MAX_ELEMENTS,
        ChartType::RR => CHART_RR_MAX_ELEMENTS,
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
        )
        .legend_position(None);
    f.render_widget(chart, area);
    // Temporarily making our own legend while we wait for Ratatui issue #1290 (https://github.com/ratatui-org/ratatui/issues/1290)
    // to allow us to change order of legend elements
    let (legend, legend_area) = bpm_rr_legend(&chart_type, area);
    f.render_widget(Clear, legend_area);
    f.render_widget(legend, legend_area);
}
