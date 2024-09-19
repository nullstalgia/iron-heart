use ratatui::{
    layout::Constraint,
    style::{Modifier, Style},
    widgets::{Block, Borders, Row, Table},
};
use ratatui_macros::row;

// TODO!
// [ ] Don't ask again
// Clickable (device menu and prompts!)
// Mouse Hover Events!

pub enum SavePromptChoice {
    Yes,
    No,
    Never,
}

/// Creates a pop-up asking if the user wants to save the device for faster connection in the future.
pub fn save_prompt() -> Table<'static> {
    // let normal_style
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);

    let rows: Vec<Row> = vec![row!["Yes"], row!["No"], row!["Never Ask Again"]];

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

// pub enum VrcxPromptChoice {
//     Yes,
//     NeverAsk,
//     OpenFolder,
// }

// pub enum UpdateCheckPromptChoice {
//     Yes,
//     No,
// }

// pub enum UpdateAppPromptChoice {
//     Yes,
//     NextTime,
//     SkipVersion,
// }

/// Creates a pop-up asking if the user wants to update to the newest version, or skip this one
/// If not portable, just offers to open a link to the changelog
// pub fn update_app_prompt(portable: bool) -> Table<'static> {}

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
