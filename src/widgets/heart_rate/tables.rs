use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table},
    Frame,
};

use crate::{app::App, heart_rate::BatteryLevel};

use ratatui_macros::{line, span};

pub fn render_table(f: &mut Frame, area: Rect, app: &App) {
    let mut rows: Vec<Row> = Vec::new();

    let mut headers = vec![
        line!["Heart Rate"],
        line!["RR (sec)"],
        line!["Battery Level"],
        line!["Session High"],
        line!["Session Low"],
    ];

    let heart_rate_status = &app.heart_rate_status;

    let battery_string: String = match heart_rate_status.battery_level {
        BatteryLevel::Unknown => "???".into(),
        BatteryLevel::NotReported => "N/A".into(),
        BatteryLevel::Level(level) => format!("{level}%"),
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

    let time_format = if app.settings.tui.session_stats_use_12hr {
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
        app.session_high_bpm.0,
        app.session_high_bpm.1.format(time_format)
    );

    let low_string = format!(
        "{:.0} BPM @ {}",
        app.session_low_bpm.0,
        app.session_low_bpm.1.format(time_format)
    );

    let mut content = vec![
        Cell::from(heart_rate_status.heart_rate_bpm.to_string()),
        Cell::from(rr_string),
        Cell::from(battery_string).style(battery_style),
        Cell::from(high_string),
        Cell::from(low_string),
    ];

    let mut constraints = vec![
        Constraint::Length(15),
        Constraint::Length(20),
        Constraint::Length(15),
        Constraint::Length(20),
        Constraint::Length(20),
    ];

    if app.settings.activities.enabled {
        headers.push(line![span!(Modifier::UNDERLINED; "A"), span!("ctivity")]);
        let activity = app.activities.selected();
        let activity: &str = activity.map(|s| s.as_str()).unwrap_or("???");
        content.push(Cell::from(activity));
        constraints.push(Constraint::Fill(1));
    }

    rows.push(Row::new(headers).style(Style::default().add_modifier(Modifier::BOLD)));
    rows.push(Row::new(content));

    let table = Table::new(rows.to_vec(), constraints)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Most Recent Data")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(table, area);
}
