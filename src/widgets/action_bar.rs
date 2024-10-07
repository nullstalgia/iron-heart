use ratatui::{
    layout::Constraint,
    style::{Color, Style},
    widgets::{Row, Table},
};
use ratatui_macros::text;
use self_update::cargo_crate_version;

pub const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

/// Creates a table with information about potential actions
pub fn action_bar(
    scan_paused: bool,
    is_loading_characteristics: bool,
    frame_count: &usize,
) -> Table<'static> {
    let index_slow = (frame_count / 2) % SPINNER.len();
    let index = frame_count % SPINNER.len();
    let info_rows = vec![Row::new(vec![
        text!["[q → exit]"],
        text!["[up/down → navigate]"],
        text!["[enter → open/close]"],
        if scan_paused {
            text!["[s → start scan]".to_string()]
        } else {
            text![format!("[s → stop scan {}]", SPINNER[index_slow])]
        },
        if is_loading_characteristics {
            text![format!("[c → connecting... {}]", SPINNER[index])]
        } else {
            text!["[c → load characteristics]".to_string()]
        },
        text![cargo_crate_version!()].right_aligned(),
    ])
    .style(Style::default().fg(Color::DarkGray))];
    let table = Table::new(
        info_rows,
        [
            Constraint::Length(10),
            Constraint::Length(20),
            Constraint::Length(20),
            Constraint::Length(17),
            Constraint::Length(30),
            Constraint::Fill(1),
        ],
    )
    .column_spacing(1);

    table
}
