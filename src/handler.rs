use std::sync::atomic::Ordering;

use crate::app::{App, ErrorPopup, SubState};
use crate::AppResult;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_input::backend::crossterm::EventHandler;

use log::*;

/// Handles the key events and updates the state of [`App`].
pub fn handle_key_events(app: &mut App, key_event: KeyEvent) -> AppResult<()> {
    // Special keys
    match key_event.code {
        KeyCode::Char('c') | KeyCode::Char('C') => {
            if key_event.modifiers == KeyModifiers::CONTROL {
                app.cancel_app.cancel();
                return Ok(());
            }
        }
        _ => {}
    }

    // Regardless of States
    match key_event.code {
        KeyCode::Esc => {
            app.escape_pressed();
        }
        KeyCode::Enter => {
            app.enter_pressed();
        }
        KeyCode::Down => {
            app.scroll_down();
        }
        KeyCode::Up => {
            app.scroll_up();
        }
        _ => {}
    }

    match app.sub_state {
        SubState::ActivitySelection => {
            app.activities
                .input
                .handle_event(&crossterm::event::Event::Key(key_event));
            app.activities.query_from_input();
        }
        _ => {
            match key_event.code {
                KeyCode::Char('e') if app.is_idle_on_ble_selection() => {
                    app.error_message = Some(ErrorPopup::UserMustDismiss(
                        "This is a test error message".to_string(),
                    ));
                    error!("This is a test error message");
                }
                KeyCode::Char('q') if app.is_idle_on_ble_selection() => {
                    app.cancel_app.cancel();
                }
                KeyCode::Char('c') | KeyCode::Char('C') if app.is_idle_on_ble_selection() => {
                    app.connect_for_characteristics();
                }
                KeyCode::Char('s') if app.is_idle_on_ble_selection() => {
                    let current_state = app.ble_scan_paused.load(Ordering::SeqCst);
                    app.ble_scan_paused.store(!current_state, Ordering::SeqCst);
                    debug!("(S) Pausing BLE scan");
                }
                KeyCode::Char('a') => {
                    app.activities.input.reset();
                    app.activities.table_state.select(None);
                    app.activities.query_from_input();
                    app.sub_state = SubState::ActivitySelection;
                }
                KeyCode::Char('j') => {
                    app.scroll_down();
                }
                KeyCode::Char('k') => {
                    app.scroll_up();
                }
                // Other handlers you could add here.
                _ => {}
            }
        }
    }
    Ok(())
}
