// use crossterm::event::{self, Event, KeyCode, KeyModifiers};

// use log::*;
// use ratatui::backend::Backend;
// use ratatui::layout::Alignment;
// use ratatui::style::Style;
// use ratatui::text::Span;
// use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
// use ratatui::{
//     layout::{Constraint, Direction, Layout},
//     Terminal,
// };
// use std::error::Error;
// use std::sync::atomic::Ordering;
// use std::time::Duration;

// use crate::app::{App, AppState, DeviceData, ErrorPopup};
// use crate::heart_rate::{HeartRateStatus, HEART_RATE_SERVICE_UUID};
// use crate::panic_handler::initialize_panic_handler;
// use crate::structs::DeviceInfo;
// use crate::utils::centered_rect;
// use crate::widgets::detail_table::detail_table;
// use crate::widgets::device_table::device_table;
// use crate::widgets::heart_rate_display::heart_rate_display;
// use crate::widgets::info_table::info_table;
// use crate::widgets::inspect_overlay::inspect_overlay;
// use crate::widgets::save_prompt::save_prompt;

// /// Displays the detected Bluetooth devices in a table and handles the user input.
// /// The user can navigate the table, pause the scanning, and quit the application.
// /// The detected devices are received through the provided `mpsc::Receiver`.
// pub async fn viewer<B: Backend>(
//     terminal: &mut Terminal<B>,
//     app: &mut App,
// ) -> Result<(), Box<dyn Error>> {
//     // Defining a custom panic hook to reset the terminal properties
//     initialize_panic_handler()?;

//     app.table_state.select(Some(0));
//     app.save_prompt_state.select(Some(0));

//     // Big loop here, drawing the different possible UIs
//     // then handing all events (keys, bt, bt -> osc | log | ui)

//     // TODO Make this shit smaller
//     loop {
//         // In case another task called for a shutdown
//         if app.shutdown_requested.is_cancelled() {
//             warn!("Viewer recieved shutdown signal!");
//             break;
//         }

//         // Draw UI
//         terminal.draw(|f| {})?;

//         // Event handling
//         if event::poll(Duration::from_millis(100))? {
//             if let Event::Key(key) = event::read()? {
//                 match key.code {
//                     _ => {}
//                 }
//             }
//         }
//     }
//     Ok(())
// }
