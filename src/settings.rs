use config::{Config, File as ConfigFile};
use log::{info, LevelFilter};
use serde_derive::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;

use crate::errors::AppError;

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct MiscSettings {
    log_level: String,
    pub write_bpm_to_file: bool,
    pub write_rr_to_file: bool,
    pub bpm_file_path: String,
    pub log_sessions_to_csv: bool,
    pub log_sessions_csv_path: String,
    pub vrcx_shortcut_prompt: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct TuiSettings {
    pub session_stats_use_12hr: bool,
    pub chart_bpm_enabled: bool,
    pub chart_rr_enabled: bool,
    pub chart_rr_max: f64,
    pub chart_rr_clamp_high: bool,
    pub chart_rr_clamp_low: bool,
    pub charts_combine: bool,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct BLESettings {
    pub never_ask_to_save: bool,
    pub saved_name: String,
    pub saved_address: String,
    pub rr_ignore_after_empty: u16,
}

// TODO Async get for osc settings due to oscquery
// and find some way to deal with the dc's/osc restarts?
// oscquery is gonna suuuck

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct OscSettings {
    pub enabled: bool,
    pub host_ip: String,
    pub target_ip: String,
    pub port: u16,
    pub pulse_length_ms: u16,
    pub only_positive_float_bpm: bool,
    pub hide_disconnections: bool,
    pub max_hide_disconnection_sec: u16,
    pub twitch_rr_threshold_ms: u16,
    pub addresses: OscAddrConf,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct OscAddrConf {
    pub prefix: String,
    pub hrm_connected: String,
    pub hiding_disconnect: String,
    pub hrm_battery_int: String,
    pub hrm_battery_float: String,
    pub beat_toggle: String,
    pub beat_pulse: String,
    pub bpm_int: String,
    pub bpm_float: String,
    pub latest_rr_int: String,
    pub rr_twitch_up: String,
    pub rr_twitch_down: String,
    pub activity: String,
    // TODO Session Max/Min/Avg Params?
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct DummySettings {
    // When enabled, BLE and Websockets are disabled
    pub enabled: bool,
    pub low_bpm: u16,
    pub high_bpm: u16,
    pub bpm_speed: f32,
    pub loops_before_dc: u16,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct WebSocketSettings {
    // Note: BLE is disabled if websockets are enabled
    pub enabled: bool,
    pub port: u16,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ActivitiesSettings {
    pub enabled: bool,
    pub remember_last: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct PrometheusSettings {
    pub enabled: bool,
    pub url: String,
    pub target_table: String,
    pub auth_type: String, // none, header, user, auth
    pub header: String,
    pub user: String,
    pub pass: String,
    pub batch_size: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct AutoUpdateSettings {
    pub update_check_prompt: bool,
    pub allow_checking_for_updates: bool,
    pub version_skipped: String,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Settings {
    pub osc: OscSettings,
    pub ble: BLESettings,
    pub websocket: WebSocketSettings,
    pub misc: MiscSettings,
    pub dummy: DummySettings,
    pub tui: TuiSettings,
    pub updates: AutoUpdateSettings,
    // pub prometheus: PrometheusSettings,
    pub activities: ActivitiesSettings,
}

impl Settings {
    #[allow(clippy::needless_late_init)]
    pub fn load(config_path: PathBuf, required: bool) -> Result<Self, AppError> {
        let default_log_level;
        let default_session_log_path;
        let default_bpm_txt_path;

        if !cfg!(debug_assertions) {
            // Release build default params
            default_log_level = "debug";
            default_session_log_path = "session_logs";
            default_bpm_txt_path = "bpm.txt"
        } else {
            // Debug build default params
            default_log_level = "debug";
            // (assuming it's in target/debug/)
            default_session_log_path = "../../session_logs";
            default_bpm_txt_path = "../../bpm.txt"
        };

        // TODO: New way of doing defaults
        // Either use serde's defaults and skip the extra config crate entirely (doesn't look like it supports serde defaults?)
        // or switch to something more sane like figment or confique
        let settings = Config::builder()
            // Start off by merging in the "default" configuration file
            .add_source(ConfigFile::from(config_path).required(required))
            .set_default("osc.enabled", true)?
            .set_default("osc.host_ip", "0.0.0.0")?
            .set_default("osc.target_ip", "127.0.0.1")?
            .set_default("osc.port", 9000)?
            .set_default("osc.pulse_length_ms", 100)?
            .set_default("osc.only_positive_float_bpm", false)?
            .set_default("osc.hide_disconnections", false)?
            .set_default("osc.max_hide_disconnection_sec", 60)?
            .set_default("osc.twitch_rr_threshold_ms", 50)?
            .set_default("osc.addresses.prefix", "/avatar/parameters/")?
            .set_default("osc.addresses.hrm_connected", "isHRConnected")?
            .set_default("osc.addresses.hiding_disconnect", "isHRReconnecting")?
            .set_default("osc.addresses.hrm_battery_int", "HRBattery")?
            .set_default("osc.addresses.hrm_battery_float", "HRBatteryFloat")?
            .set_default("osc.addresses.beat_toggle", "HeartBeatToggle")?
            .set_default("osc.addresses.beat_pulse", "isHRBeat")?
            .set_default("osc.addresses.bpm_int", "HR")?
            .set_default("osc.addresses.bpm_float", "floatHR")?
            .set_default("osc.addresses.latest_rr_int", "RRInterval")?
            .set_default("osc.addresses.rr_twitch_up", "HRTwitchUp")?
            .set_default("osc.addresses.rr_twitch_down", "HRTwitchDown")?
            .set_default("osc.addresses.activity", "HRActivity")?
            .set_default("ble.never_ask_to_save", false)?
            .set_default("ble.saved_address", "")?
            .set_default("ble.saved_name", "")?
            .set_default("ble.rr_ignore_after_empty", 0)?
            .set_default("websocket.enabled", false)?
            .set_default("websocket.port", 5566)?
            .set_default("misc.log_level", default_log_level)?
            .set_default("misc.write_bpm_to_file", false)?
            .set_default("misc.write_rr_to_file", false)?
            .set_default("misc.bpm_file_path", default_bpm_txt_path)?
            .set_default("misc.log_sessions_to_csv", false)?
            .set_default("misc.log_sessions_csv_path", default_session_log_path)?
            .set_default("misc.vrcx_shortcut_prompt", true)?
            .set_default("updates.update_check_prompt", true)?
            .set_default("updates.allow_checking_for_updates", false)?
            .set_default("updates.version_skipped", "")?
            .set_default("tui.session_stats_use_12hr", true)?
            .set_default("tui.chart_bpm_enabled", true)?
            .set_default("tui.chart_rr_enabled", true)?
            .set_default("tui.chart_rr_max", 2.0)?
            .set_default("tui.chart_rr_clamp_high", true)?
            .set_default("tui.chart_rr_clamp_low", false)?
            .set_default("tui.charts_combine", true)?
            .set_default("dummy.enabled", false)?
            .set_default("dummy.low_bpm", 50)?
            .set_default("dummy.high_bpm", 120)?
            .set_default("dummy.bpm_speed", 1.5)?
            .set_default("dummy.loops_before_dc", 2)?
            .set_default("activities.enabled", false)?
            .set_default("activities.remember_last", true)?
            // .set_default("prometheus.enabled", false)?
            // .set_default("prometheus.url", "")?
            // .set_default("prometheus.auth_type", "none")?
            // .set_default("prometheus.header", "")?
            // .set_default("prometheus.user", "")?
            // .set_default("prometheus.pass", "")?
            // .set_default("prometheus.batch_size", 30)?
            .build()?
            .try_deserialize()?;

        Ok(settings)
    }

    // TODO look into blank configs being saved on crash?
    // Maybe new blank name/address check fixes it, unsure yet.
    pub fn save(&self, config_path: &PathBuf) -> Result<(), AppError> {
        // TODO Look into toml_edit's options
        let toml_config = toml::to_string(self)?;

        info!("Serialized config length: {}", toml_config.len());

        let mut file = File::create(config_path).map_err(|e| AppError::CreateFile {
            path: PathBuf::from(config_path),
            source: e,
        })?;

        file.write_all(toml_config.as_bytes())
            .map_err(|e| AppError::WriteFile {
                path: PathBuf::from(config_path),
                source: e,
            })?;

        file.flush().map_err(|e| AppError::WriteFile {
            path: PathBuf::from(config_path),
            source: e,
        })?;

        file.sync_data().map_err(|e| AppError::WriteFile {
            path: PathBuf::from(config_path),
            source: e,
        })?;

        Ok(())
    }
    pub fn get_log_level(&self) -> LevelFilter {
        LevelFilter::from_str(&self.misc.log_level).unwrap_or(LevelFilter::Info)
    }
}
