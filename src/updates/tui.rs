use ratatui::{
    layout::{Alignment, Constraint, Layout},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, Gauge, Paragraph, Row, Table, Wrap},
};

use ratatui::prelude::*;
use ratatui_macros::{row, text};

use num_enum::FromPrimitive;
use self_update::cargo_crate_version;

use crate::{app::App, is_portable, utils::centered_rect};

pub fn update_allow_check_prompt(app: &mut App, frame: &mut Frame) {
    let area = centered_rect(65, 60, frame.area());

    // Create the outer block with borders and title
    let block = Block::default()
        .borders(Borders::ALL)
        //.light_green()
        .title("Auto Update");

    // Get the inner area of the block
    let vertical = Layout::vertical([Constraint::Fill(2), Constraint::Min(4)]);
    let inner_area = block.inner(area);
    let [explanation, options] = vertical.areas(inner_area);

    // Create a paragraph explaining autoconnect
    let explanation_paragraph = Paragraph::new(text!["Allow checking for updates on app startup?"])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, inner_area);
    frame.render_widget(block, area);
    frame.render_widget(explanation_paragraph, explanation);
    frame.render_stateful_widget(check_options(), options, &mut app.prompt_state);
}

// I need to make a macro for these >:C
// But I don't wanna deal with having an extra crate, bleh

#[derive(Debug, Eq, PartialEq, FromPrimitive)]
#[repr(u8)]
pub enum UpdateCheckChoice {
    Yes,
    #[num_enum(default)]
    No,
    NeverAsk,
}
pub fn check_options() -> Table<'static> {
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let rows: Vec<Row> = vec![
        row!["Yes"],
        row!["No, ask later"],
        row!["No, and don't ask again"],
    ];
    let option_table = Table::new(rows, [Constraint::Percentage(100)])
        .highlight_style(selected_style)
        .highlight_symbol(">> ");
    option_table
}

#[derive(Debug, Eq, PartialEq, FromPrimitive)]
#[repr(u8)]
pub enum UpdatePromptChoice {
    Yes,
    OpenRepository,
    #[num_enum(default)]
    No,
    SkipVersion,
}
pub fn update_now_options() -> Table<'static> {
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let download_option = if is_portable() {
        row!["Download and install"]
    } else {
        row![""]
    };
    let rows: Vec<Row> = vec![
        download_option,
        row!["Open GitHub Repository"],
        row!["Ask again later"],
        row!["Skip this version"],
    ];

    let option_table = Table::new(rows, [Constraint::Percentage(100)])
        .highlight_style(selected_style)
        .highlight_symbol(">> ");
    option_table
}

pub fn update_found_prompt(app: &mut App, frame: &mut Frame) {
    let area = centered_rect(65, 60, frame.area());

    // Create the outer block with borders and title
    let block = Block::default()
        .borders(Borders::ALL)
        // .light_green()
        .title("Update Found!");

    // Get the inner area of the block
    let vertical = Layout::vertical([Constraint::Fill(2), Constraint::Min(4)]);
    let inner_area = block.inner(area);
    let [explanation, options] = vertical.areas(inner_area);

    let current_version = cargo_crate_version!();
    let new_version = app.update_newer_version.as_deref().unwrap_or("???");

    let download_prompt = if is_portable() {
        "Would you like to download it now?"
    } else {
        "Would you like to open the repository in your web browser?"
    };

    // Create a paragraph explaining autoconnect
    let explanation_paragraph = Paragraph::new(text![
        "A newer version of the application has been released!",
        format!("{current_version} -> {new_version}"),
        "",
        download_prompt
    ])
    .alignment(Alignment::Center)
    .wrap(Wrap { trim: true });

    frame.render_widget(Clear, inner_area);
    frame.render_widget(block, area);
    frame.render_widget(explanation_paragraph, explanation);
    frame.render_stateful_widget(update_now_options(), options, &mut app.prompt_state);
}

pub fn update_downloading_ui(app: &mut App, frame: &mut Frame) {
    let mut area = centered_rect(65, 60, frame.area());
    area.height = area.height.min(5);

    // Create the outer block with borders and title
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().light_blue())
        .title("Update Downloading...");

    // Get the inner area of the block
    let inner_area = block.inner(area);

    let gauge = Gauge::default()
        .ratio(app.update_download_percentage)
        .light_blue();

    frame.render_widget(Clear, inner_area);
    frame.render_widget(block, area);
    frame.render_widget(gauge, inner_area);
}

#[cfg(windows)]
#[derive(Debug, Eq, PartialEq, FromPrimitive)]
#[repr(u8)]
pub enum UpdateRestartChoice {
    Yes,
    #[num_enum(default)]
    No,
}
#[cfg(windows)]
pub fn restart_options() -> Table<'static> {
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    // If we're not portable, we just want to open a link to the repo, let the user handle the update via however they installed it.
    let rows: Vec<Row> = vec![row!["Launch in new window"], row!["No, close"]];
    let option_table = Table::new(rows, [Constraint::Percentage(100)])
        .highlight_style(selected_style)
        .highlight_symbol(">> ");
    option_table
}
#[cfg(windows)]
pub fn restart_app_prompt(app: &mut App, frame: &mut Frame) {
    let mut area = centered_rect(65, 60, frame.area());
    area.height = area.height.min(10);

    // Create the outer block with borders and title
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().light_green())
        .title("Update Complete!");

    // Get the inner area of the block
    let vertical = Layout::vertical([Constraint::Fill(2), Constraint::Min(4)]);
    let inner_area = block.inner(area);
    let [explanation, options] = vertical.areas(inner_area);

    let explanation_paragraph = Paragraph::new(text!["Done! Launch updated app in new window?"])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, inner_area);
    frame.render_widget(block, area);
    frame.render_widget(explanation_paragraph, explanation);
    frame.render_stateful_widget(restart_options(), options, &mut app.prompt_state);
}
