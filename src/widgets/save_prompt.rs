use ratatui::{
    layout::Constraint,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Row, Table},
};

use crate::{structs::DeviceInfo, utils::extract_manufacturer_data};

/// Creates a pop-up asking if the user wants to save the device for faster connection in the future.
pub fn save_prompt(selected: Option<usize>, selected_device: &DeviceInfo) -> Table {
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);



    // Options: Yes, No, Never
    let rows: Vec<Row>

    let option_table = Table::new(
        rows,
        [
            Constraint::Length(40),
            Constraint::Length(20),
            Constraint::Length(30),
            Constraint::Length(10),
        ],
    )
    .block(Block::default().title("Save device?").borders(Borders::ALL))
    .highlight_style(selected_style);

    table
}
