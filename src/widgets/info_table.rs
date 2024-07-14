use ratatui::{
    layout::Constraint,
    style::{Color, Style},
    widgets::{Row, Table},
};

pub const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

/// Creates a table with information about the application and the user input.
pub fn info_table(
    scan_paused: bool,
    is_loading_characteristics: &bool,
    frame_count: &usize,
) -> Table<'static> {
    //let spinner = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let index_slow = (frame_count / 2) % SPINNER.len();
    let index = frame_count % SPINNER.len();
    let info_rows = vec![Row::new(vec![
        "[q → exit]".to_string(),
        "[up/down → navigate]".to_string(),
        "[enter → open/close]".to_string(),
        if scan_paused {
            "[s → start scan]".to_string()
        } else {
            format!("[s → stop scan {}]", SPINNER[index_slow])
        },
        if *is_loading_characteristics {
            format!("[c → loading... {}]", SPINNER[index])
        } else {
            "[c → load characteristics]".to_string()
        },
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
        ],
    )
    .column_spacing(1);

    table
}
