use std::collections::HashMap;

use ratatui::layout::Rect;

use crate::{company_codes::COMPANY_CODE, structs::ManufacturerData};

/// Extracts the manufacturer data from a `HashMap<u16, Vec<u8>>` and returns a tuple with the company name and the manufacturer data as a string.
/// If the manufacturer data is empty, it returns "n/a" as the company name and the manufacturer data.
/// If the company code is not found in the `company_codes` module, it returns "n/a" as the company name.
pub fn extract_manufacturer_data(manufacturer_data: &HashMap<u16, Vec<u8>>) -> ManufacturerData {
    let mut c = None;
    let mut m = manufacturer_data
        .iter()
        .map(|(&key, value)| {
            c = Some(key);
            let hex_string = value
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<String>>()
                .join(" ");
            hex_string.to_string()
        })
        .collect::<Vec<String>>()
        .join(" ");
    m = if m.is_empty() { "n/a".to_string() } else { m };
    match c {
        Some(code) => ManufacturerData {
            company_code: COMPANY_CODE.get(&code).unwrap_or(&"n/a").to_string(),
            data: m,
        },
        None => ManufacturerData {
            company_code: "n/a".to_string(),
            data: m,
        },
    }
}

/// Returns a `Rect` with the provided percentage of the parent `Rect` and centered.
pub fn centered_rect(percent_x: u16, percent_y: u16, size: Rect) -> Rect {
    let popup_size = Rect {
        width: size.width * percent_x / 100,
        height: size.height * percent_y / 100,
        ..Rect::default()
    };
    Rect {
        x: (size.width - popup_size.width) / 2,
        y: (size.height - popup_size.height) / 2,
        ..popup_size
    }
}
