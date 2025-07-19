use num_enum::FromPrimitive;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table, Wrap},
    Frame,
};
use ratatui_macros::{row, span};

use crate::{
    app::{App, ErrorPopup},
    utils::centered_rect,
};

// TODO!
// [ ] Don't ask again
// Clickable (device menu and prompts!)
// Mouse Hover Events!

#[derive(Debug, Eq, PartialEq, FromPrimitive)]
#[repr(u8)]
pub enum SavePromptChoice {
    Yes,
    #[num_enum(default)]
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

pub fn connecting_popup<'a>(
    device_name: &str,
    device_mac: &str,
    quick_connect_ui: bool,
) -> Paragraph<'a> {
    let mut name = device_name;
    let mut border_style = Style::default();

    // Set border to green if we're quick-connecting.
    if quick_connect_ui {
        border_style = Style::default().fg(ratatui::style::Color::Green);
        if name == "Unknown" {
            name = "Saved Device";
        }
    }

    Paragraph::new(format!("Connecting to:\n{name}\n({device_mac})"))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style),
        )
}

pub fn render_error_popup(app: &App, f: &mut Frame) {
    if let Some(error_message) = app.error_message.as_ref() {
        let (style, message, error_details, title) = match error_message {
            ErrorPopup::FatalDetailed(msg, error) => (
                Style::default().fg(ratatui::style::Color::Red),
                msg,
                Some(error),
                "!! Error !!",
            ),
            ErrorPopup::Fatal(msg) => (
                Style::default().fg(ratatui::style::Color::Red),
                msg,
                None,
                "!! Error !!",
            ),
            ErrorPopup::Intermittent(msg) => (
                Style::default().fg(ratatui::style::Color::Yellow),
                msg,
                None,
                "Warning",
            ),
            ErrorPopup::UserMustDismiss(msg) => (
                Style::default().fg(ratatui::style::Color::Blue),
                msg,
                None,
                "!! Notification !!",
            ),
        };

        let area = centered_rect(60, 50, f.area());

        // Create the outer block with borders and title
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(style);

        // Draw the block
        f.render_widget(Clear, area);
        f.render_widget(&block, area);

        // Get the inner area of the block
        let inner_area = block.inner(area);

        // Special layout for when our error might have pretty-printed info
        if let Some(error_message) = error_details {
            // Split the inner_area vertically into two
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Fill(1), Constraint::Fill(3)].as_ref())
                .split(inner_area);

            let first_paragraph = Paragraph::new(span!(message))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true });

            let lines: Vec<Line> = error_message.lines().map(Line::raw).collect();
            let details = Text::from(lines);
            let second_paragraph = Paragraph::new(details)
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: false });

            // Render the paragraphs
            f.render_widget(first_paragraph, chunks[0]);
            f.render_widget(second_paragraph, chunks[1]);
        } else {
            // Standard message text rendering
            let error_paragraph = Paragraph::new(span!(message))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true });

            f.render_widget(error_paragraph, inner_area);
        }
    }
}

// pub enum UpdateCheckPromptChoice {
//     Yes,
//     No,
// }

// pub enum UpdateAppPromptChoice {
//     Yes,
//     NextTime,
//     SkipVersion,
// }

// Creates a pop-up asking if the user wants to update to the newest version, or skip this one
// If not portable, just offers to open a link to the changelog
// pub fn update_app_prompt(portable: bool) -> Table<'static> {}
