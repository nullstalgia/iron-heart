use ratatui::{
    layout::{Alignment, Constraint},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Row, Table},
};

pub enum SavePromptChoice {
    Yes,
    No,
    Never,
}

impl From<usize> for SavePromptChoice {
    fn from(index: usize) -> Self {
        match index {
            0 => SavePromptChoice::Yes,
            1 => SavePromptChoice::No,
            2 => SavePromptChoice::Never,
            _ => SavePromptChoice::No,
        }
    }
}

/// Creates a pop-up asking if the user wants to save the device for faster connection in the future.
pub fn save_prompt() -> Table<'static> {
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
