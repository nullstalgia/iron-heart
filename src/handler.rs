use std::sync::atomic::Ordering;

use crate::app::{App, AppState, ErrorPopup};
use crate::AppResult;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use log::*;

/// Handles the key events and updates the state of [`App`].
pub fn handle_key_events(app: &mut App, key_event: KeyEvent) -> AppResult<()> {
    match key_event.code {
        KeyCode::Char('e') if app.is_idle_on_main_menu() => {
            app.error_message = Some(ErrorPopup::UserMustDismiss(
                "This is a test error message".to_string(),
            ));
            error!("This is a test error message");
        }
        KeyCode::Char('q') if app.is_idle_on_main_menu() => {
            app.cancel_app.cancel();
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            if key_event.modifiers == KeyModifiers::CONTROL {
                app.cancel_app.cancel();
            } else if app.is_idle_on_main_menu() {
                app.connect_for_characteristics();
            }
        }
        KeyCode::Char('s') if app.is_idle_on_main_menu() => {
            let current_state = app.ble_scan_paused.load(Ordering::SeqCst);
            app.ble_scan_paused.store(!current_state, Ordering::SeqCst);
            debug!("(S) Pausing BLE scan");
        }
        KeyCode::Enter => {
            app.enter_pressed();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.scroll_down();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.scroll_up();
        }
        // Other handlers you could add here.
        _ => {}
    }
    Ok(())
}
