use std::collections::HashMap;

use ratatui::{
    layout::Constraint,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Row, Table},
};

use crate::structs::Characteristic;

/// Provides an overlay with the selected device's services.
pub fn inspect_overlay(
    characteristics: &[Characteristic],
    scroll: usize,
    height: u16,
) -> Table<'static> {
    let mut rows: Vec<Row> = Vec::new();
    let mut services: HashMap<String, Vec<&Characteristic>> = HashMap::new();

    if characteristics.is_empty() {
        rows.push(Row::new(vec!["Loading..."]));
        return Table::new(rows, [Constraint::Percentage(100)])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Characteristics")
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    }

    for characteristic in characteristics.iter() {
        let service_uuid = characteristic.service.to_string();
        services
            .entry(service_uuid)
            .or_default()
            .push(characteristic);
    }

    let mut sorted_services: Vec<_> = services.into_iter().collect();
    sorted_services.sort_by_key(|(uuid, _)| uuid.clone());

    for (service_uuid, characteristics) in sorted_services {
        rows.push(
            Row::new(vec![format!("Service: {service_uuid}")])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        );

        for characteristic in characteristics {
            let properties = format!(
                "{:?}",
                characteristic
                    .properties
                    .iter_names()
                    .map(|x| x.0.to_string())
                    .collect::<Vec<String>>()
                    .join(", ")
            );

            rows.push(Row::new(vec![format!(
                "  ↳ Characteristic: {}",
                characteristic.uuid.to_string()
            )]));
            rows.push(Row::new(vec![format!("    ↳ Properties: {}", properties)]));

            for descriptor in characteristic.descriptors.iter() {
                rows.push(Row::new(vec![format!(
                    "    ↳ Descriptor: {}",
                    descriptor.to_string()
                )]));
            }
        }
    }

    let adjusted_height = if height > 3 { height - 3 } else { height };
    let visible_rows_count = adjusted_height as usize;
    let total_rows = rows.len();
    let start_index = scroll;
    let end_index = usize::min(start_index + visible_rows_count, total_rows);
    let visible_rows = if start_index < total_rows {
        &rows[start_index..end_index]
    } else {
        &[]
    };

    Table::new(visible_rows.to_vec(), [Constraint::Percentage(100)])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Characteristics")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
}
