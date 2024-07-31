use ratatui::{
    layout::{Constraint, Layout},
    Frame,
};

use crate::{
    app::App,
    widgets::heart_rate::{
        charts::{render_bpm_chart, render_combined_chart, render_rr_chart},
        tables::render_table,
    },
};

// TODO Ascii Heart Beat Animation

pub const CHART_BPM_MAX_ELEMENTS: usize = 120;
pub const CHART_RR_MAX_ELEMENTS: usize = 120;
pub const CHART_BPM_VERT_MARGIN: f64 = 3.0;
pub const CHART_RR_VERT_MARGIN: f64 = 1.0;

pub fn heart_rate_display(frame: &mut Frame, app: &App) {
    let area = frame.size();

    let vertical = Layout::vertical([Constraint::Min(4), Constraint::Percentage(100)]);
    let horizontal_shared = Layout::horizontal([Constraint::Percentage(100)]);
    let horizontal_split =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]);
    let [status_area, bottom] = vertical.areas(area);
    let [bpm_history, rr_history] = horizontal_split.areas(bottom);
    let [shared_chart] = horizontal_shared.areas(bottom);

    render_table(
        frame,
        status_area,
        &app.heart_rate_status,
        &app.session_high_bpm,
        &app.session_low_bpm,
        app.settings.misc.session_stats_use_12hr,
    );
    let hr_chart = app.settings.misc.session_chart_hr_enabled;
    let rr_chart = app.settings.misc.session_chart_rr_enabled;
    let combined = app.settings.misc.session_charts_combine;

    if combined && hr_chart && rr_chart {
        render_combined_chart(
            frame,
            shared_chart,
            &app.heart_rate_history,
            &app.rr_history,
            &app.session_high_bpm,
            &app.session_low_bpm,
            &app.session_high_rr,
            &app.session_low_rr,
        );
    } else {
        if hr_chart && rr_chart {
            render_bpm_chart(
                frame,
                bpm_history,
                &app.heart_rate_history,
                &app.session_high_bpm,
                &app.session_low_bpm,
            );
            render_rr_chart(
                frame,
                rr_history,
                &app.rr_history,
                &app.session_high_rr,
                &app.session_low_rr,
            );
        } else if hr_chart {
            render_bpm_chart(
                frame,
                shared_chart,
                &app.heart_rate_history,
                &app.session_high_bpm,
                &app.session_low_bpm,
            );
        } else if rr_chart {
            render_rr_chart(
                frame,
                shared_chart,
                &app.rr_history,
                &app.session_high_rr,
                &app.session_low_rr,
            );
        }
    }
}
