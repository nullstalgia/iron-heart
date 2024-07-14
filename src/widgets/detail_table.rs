use ratatui::{
    layout::Constraint,
    widgets::{Block, Borders, Row, Table},
};

use crate::{structs::DeviceInfo, utils::extract_manufacturer_data};

/// Creates a table with more detailed information about a selected device.
pub fn detail_table(selected_device: &DeviceInfo) -> Table {
    let services_binding = selected_device.services.len().to_string();
    let manufacturer_data = extract_manufacturer_data(&selected_device.manufacturer_data);
    let table = Table::new(
        vec![
            Row::new(vec![
                "Last Update At:".to_owned(),
                selected_device.updated_at.clone(),
            ]),
            Row::new(vec!["Services:".to_owned(), services_binding]),
            Row::new(vec![
                "Company Code ID:".to_owned(),
                manufacturer_data.company_code,
            ]),
            // This changes every time DeviceUpdate is called???
            // Row::new(vec![
            //     "Manufacturer Data:".to_owned(),
            //     manufacturer_data.data,
            // ]),
        ],
        [Constraint::Length(20), Constraint::Length(80)],
    )
    .block(
        Block::default()
            .title("More Details".to_owned())
            .borders(Borders::ALL),
    );

    table
}
