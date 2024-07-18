use ratatui::{
    layout::{Alignment, Constraint},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Row, Table},
};

use crate::{structs::DeviceInfo, utils::extract_manufacturer_data};

/// Creates a pop-up asking if the user wants to save the device for faster connection in the future.
pub fn save_prompt(selected: Option<usize>, selected_device: &DeviceInfo) -> Table {
    // let normal_style
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);

    let rows: Vec<Row> = vec![
        Row::new(vec![Line::from("Yes").alignment(Alignment::Left)]),
        Row::new(vec![Line::from("No").alignment(Alignment::Left)]),
        Row::new(vec![
            Line::from("Never Ask Again").alignment(Alignment::Left)
        ]),
    ];

    let option_table = Table::new(rows, [Constraint::Percentage(100)])
        .block(
            Block::default()
                .title("Autoconnect to this device?")
                .borders(Borders::ALL),
        )
        .highlight_style(selected_style)
        .highlight_symbol(">> ");

    option_table
}
