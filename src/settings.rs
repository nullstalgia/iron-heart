use config::{Config, File as ConfigFile};
use log::LevelFilter;
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
    pub address_prefix: String,
    pub param_hrm_connected: String,
    pub param_hiding_disconnect: String,
    pub param_hrm_battery_int: String,
    pub param_hrm_battery_float: String,
    pub param_beat_toggle: String,
    pub param_beat_pulse: String,
    pub param_bpm_int: String,
    pub param_bpm_float: String,
    pub param_latest_rr_int: String,
    pub twitch_rr_threshold_ms: u16,
    pub param_rr_twitch_up: String,
    pub param_rr_twitch_down: String,
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

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Settings {
    pub osc: OscSettings,
    pub ble: BLESettings,
    pub websocket: WebSocketSettings,
    pub misc: MiscSettings,
    pub dummy: DummySettings,
}

impl Settings {
    pub fn load(config_path: PathBuf) -> Result<Self, AppError> {
        let default_log_level;
        let default_session_log_path;
        let default_bpm_txt_path;

        if !cfg!(debug_assertions) {
            // Release build default params
            default_log_level = "info";
            default_session_log_path = "session_logs";
            default_bpm_txt_path = "bpm.txt"
        } else {
            // Debug build default params
            default_log_level = "debug";
            // (assuming it's in target/debug/)
            default_session_log_path = "../../session_logs";
            default_bpm_txt_path = "../../bpm.txt"
        };

        let settings = Config::builder()
            // Start off by merging in the "default" configuration file
            .add_source(ConfigFile::from(config_path).required(false))
            .set_default("osc.enabled", true)?
            .set_default("osc.host_ip", "0.0.0.0")?
            .set_default("osc.target_ip", "127.0.0.1")?
            .set_default("osc.port", 9000)?
            .set_default("osc.pulse_length_ms", 100)?
            .set_default("osc.only_positive_float_bpm", false)?
            .set_default("osc.address_prefix", "/avatar/parameters/")?
            .set_default("osc.hide_disconnections", false)?
            .set_default("osc.max_hide_disconnection_sec", 60)?
            .set_default("osc.param_hrm_connected", "isHRConnected")?
            .set_default("osc.param_hiding_disconnect", "isHRReconnecting")?
            .set_default("osc.param_hrm_battery_int", "HRBattery")?
            .set_default("osc.param_hrm_battery_float", "HRBatteryFloat")?
            .set_default("osc.param_beat_toggle", "HeartBeatToggle")?
            .set_default("osc.param_beat_pulse", "isHRBeat")?
            .set_default("osc.param_bpm_int", "HR")?
            .set_default("osc.param_bpm_float", "floatHR")?
            .set_default("osc.param_latest_rr_int", "RRInterval")?
            .set_default("osc.twitch_rr_threshold_ms", 50)?
            .set_default("osc.param_rr_twitch_up", "HRTwitchUp")?
            .set_default("osc.param_rr_twitch_down", "HRTwitchDown")?
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
            .set_default("misc.session_stats_use_12hr", true)?
            .set_default("misc.chart_bpm_enabled", true)?
            .set_default("misc.chart_rr_enabled", true)?
            .set_default("misc.chart_rr_max", 2.0)?
            .set_default("misc.chart_rr_clamp_high", true)?
            .set_default("misc.chart_rr_clamp_low", false)?
            .set_default("misc.charts_combine", true)?
            .set_default("dummy.enabled", false)?
            .set_default("dummy.low_bpm", 50)?
            .set_default("dummy.high_bpm", 120)?
            .set_default("dummy.bpm_speed", 1.5)?
            .set_default("dummy.loops_before_dc", 2)?
            .build()?
            .try_deserialize()?;

        Ok(settings)
    }

    pub fn save(&self, config_path: &PathBuf) -> Result<(), AppError> {
        let toml_config = toml::to_string(self)?;

        let mut file = File::create(config_path)?;
        file.write_all(toml_config.as_bytes())?;

        Ok(())
    }
    pub fn get_log_level(&self) -> LevelFilter {
        LevelFilter::from_str(&self.misc.log_level).unwrap_or(LevelFilter::Info)
    }
}
