use ratatui::{
    layout::{Alignment, Constraint, Layout},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table, Wrap},
};

use ratatui::prelude::*;
use ratatui_macros::{line, row, text};

use crate::{app::App, utils::centered_rect};

pub fn vrcx_prompt(app: &mut App, frame: &mut Frame) {
    let area = centered_rect(65, 60, frame.area());

    // Create the outer block with borders and title
    let block = Block::default()
        .borders(Borders::ALL)
        //.light_green()
        .title("VRCX Detected!")
        .title_style(Style::new().green());

    // Get the inner area of the block
    let vertical = Layout::vertical([Constraint::Fill(2), Constraint::Min(4)]);
    let inner_area = block.inner(area);
    let [explanation, options] = vertical.areas(inner_area);

    // Create a paragraph explaining autoconnect
    let explanation_paragraph = Paragraph::new(text![
        "VRCX can autostart this application alongside VRChat!",
        "Would you like me to generate the required shortcut?",
        "(It's recommended to move the app out of your Downloads folder first!)",
        "",
        line![
            "Note: Old startup shortcuts can be modified with ",
            "Open Startup Folder".bold().blue()
        ]
    ])
    .alignment(Alignment::Center)
    .wrap(Wrap { trim: true });

    frame.render_widget(Clear, inner_area);
    frame.render_widget(block, area);
    frame.render_widget(explanation_paragraph, explanation);
    frame.render_stateful_widget(vrcx_options(), options, &mut app.prompt_state);
}

/// Creates a pop-up asking if the user wants to save the device for faster connection in the future.
pub fn vrcx_options() -> Table<'static> {
    // let normal_style
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);

    let rows: Vec<Row> = vec![
        row!["Yes"],
        row!["No, ask later"],
        row!["No, and don't ask again"],
        row!["Open Startup Folder".blue()],
    ];

    let option_table = Table::new(rows, [Constraint::Percentage(100)])
        .highlight_style(selected_style)
        .highlight_symbol(">> ");

    option_table
}

pub enum VrcxPromptChoice {
    Yes,
    No,
    NeverAsk,
    OpenFolder,
}

impl From<usize> for VrcxPromptChoice {
    fn from(choice: usize) -> Self {
        match choice {
            0 => Self::Yes,
            1 => Self::No,
            2 => Self::NeverAsk,
            3 => Self::OpenFolder,
            _ => Self::No,
        }
    }
}
