use chrono::{DateTime, Local};
use log::*;
use ratatui::widgets::TableState;
use std::collections::VecDeque;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{
    mpsc::{self, UnboundedReceiver, UnboundedSender},
    Mutex,
};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::heart_rate_dummy::start_dummy_thread;
use crate::{
    heart_rate::{start_notification_thread, HeartRateStatus},
    logging::logging_thread,
    osc::osc_thread,
    scan::{bluetooth_event_thread, get_characteristics},
    settings::Settings,
    structs::{Characteristic, DeviceInfo},
    widgets::heart_rate_display::{
        CHART_BPM_MAX_ELEMENTS, CHART_BPM_VERT_MARGIN, CHART_RR_MAX_ELEMENTS, CHART_RR_VERT_MARGIN,
    },
};

pub enum DeviceData {
    ConnectedEvent(String),
    DisconnectedEvent(String),
    DeviceInfo(DeviceInfo),
    Characteristics(Vec<Characteristic>),
    HeartRateStatus(HeartRateStatus),
    Error(ErrorPopup),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AppState {
    MainMenu,
    CharacteristicView,
    SaveDevicePrompt,
    ConnectingForHeartRate,
    ConnectingForCharacteristics,
    HeartRateView,
    HeartRateViewNoData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorPopup {
    Intermittent(String),
    UserMustDismiss(String),
    Fatal(String),
}

pub struct App {
    // Devices as found by the BLE thread
    pub ble_rx: UnboundedReceiver<DeviceData>,
    pub ble_tx: UnboundedSender<DeviceData>,
    // BLE Notifications from the heart rate monitor
    pub hr_rx: UnboundedReceiver<DeviceData>,
    pub hr_tx: UnboundedSender<DeviceData>,
    // Sending data to the OSC thread
    pub osc_rx: Arc<Mutex<UnboundedReceiver<HeartRateStatus>>>,
    pub osc_tx: UnboundedSender<HeartRateStatus>,
    // Sending data to the logging thread
    pub log_rx: Arc<Mutex<UnboundedReceiver<HeartRateStatus>>>,
    pub log_tx: UnboundedSender<HeartRateStatus>,
    pub ble_scan_paused: Arc<AtomicBool>,
    pub state: AppState,
    pub table_state: TableState,
    pub save_prompt_state: TableState,
    pub allow_saving: bool,
    // devices with the heart rate service
    // UI references this using table_state as the index
    pub discovered_devices: Vec<DeviceInfo>,
    pub quick_connect_ui: bool,
    pub characteristic_scroll: usize,
    pub selected_characteristics: Vec<Characteristic>,
    pub frame_count: usize,
    pub error_message: Option<ErrorPopup>,
    pub settings: Settings,
    pub heart_rate_status: HeartRateStatus,
    pub shutdown_requested: CancellationToken,
    pub ble_thread_handle: Option<tokio::task::JoinHandle<()>>,
    pub hr_thread_handle: Option<tokio::task::JoinHandle<()>>,
    pub osc_thread_handle: Option<tokio::task::JoinHandle<()>>,
    pub logging_thread_handle: Option<tokio::task::JoinHandle<()>>,
    pub dummy_thread_handle: Option<tokio::task::JoinHandle<()>>,
    // Raw histories
    pub heart_rate_history: VecDeque<f64>,
    pub rr_history: VecDeque<f64>,
    // Used for the graphs in the heart rate view
    pub bpm_dataset: Vec<(f64, f64)>,
    pub rr_dataset: Vec<(f64, f64)>,
    pub session_high_bpm: (f64, DateTime<Local>),
    pub session_low_bpm: (f64, DateTime<Local>),
    // Usually same as session but can have a margin applied
    pub chart_high_bpm: f64,
    pub chart_mid_bpm: f64,
    pub chart_low_bpm: f64,

    pub chart_high_rr: f64,
    pub chart_mid_rr: f64,
    pub chart_low_rr: f64,
}

impl App {
    pub fn new() -> Self {
        let (app_tx, app_rx) = mpsc::unbounded_channel();
        let (hr_tx, hr_rx) = mpsc::unbounded_channel();
        let (osc_tx, osc_rx) = mpsc::unbounded_channel();
        let (log_tx, log_rx) = mpsc::unbounded_channel();
        let mut error_message = None;
        let settings = Settings::new().unwrap_or_else(|err| {
            warn!("Failed to load settings: {}", err);
            error_message = Some(ErrorPopup::Fatal(
                "Failed to load settings! Please fix file or delete to regenerate.".to_string(),
            ));
            Settings::default()
        });
        Self {
            ble_tx: app_tx,
            ble_rx: app_rx,
            hr_tx,
            hr_rx,
            osc_tx,
            osc_rx: Arc::new(Mutex::new(osc_rx)),
            log_tx,
            log_rx: Arc::new(Mutex::new(log_rx)),
            ble_scan_paused: Arc::new(AtomicBool::default()),
            state: AppState::MainMenu,
            table_state: TableState::default(),
            save_prompt_state: TableState::default(),
            allow_saving: false,
            discovered_devices: Vec::new(),
            quick_connect_ui: false,
            characteristic_scroll: 0,
            selected_characteristics: Vec::new(),
            frame_count: 0,
            error_message,
            settings,
            heart_rate_status: HeartRateStatus::default(),
            heart_rate_history: VecDeque::with_capacity(CHART_BPM_MAX_ELEMENTS),
            rr_history: VecDeque::with_capacity(CHART_RR_MAX_ELEMENTS),
            bpm_dataset: Vec::with_capacity(CHART_BPM_MAX_ELEMENTS),
            rr_dataset: Vec::with_capacity(CHART_RR_MAX_ELEMENTS),
            shutdown_requested: CancellationToken::new(),
            ble_thread_handle: None,
            hr_thread_handle: None,
            osc_thread_handle: None,
            logging_thread_handle: None,
            dummy_thread_handle: None,
            session_high_bpm: (0.0, Local::now()),
            session_low_bpm: (0.0, Local::now()),
            chart_high_bpm: 0.0,
            chart_low_bpm: 0.0,
            chart_mid_bpm: 0.0,
            chart_high_rr: 0.0,
            chart_low_rr: 0.0,
            chart_mid_rr: 0.0,
        }
    }

    pub async fn start_bluetooth_event_thread(&mut self) {
        let pause_signal_clone = Arc::clone(&self.ble_scan_paused);
        let app_tx_clone = self.ble_tx.clone();
        let shutdown_requested_clone = self.shutdown_requested.clone();
        debug!("Spawning Bluetooth CentralEvent thread");
        self.ble_thread_handle = Some(tokio::spawn(async move {
            bluetooth_event_thread(app_tx_clone, pause_signal_clone, shutdown_requested_clone).await
        }));
    }

    pub async fn connect_for_characteristics(&mut self) {
        if self.discovered_devices.is_empty() {
            return;
        }
        let selected_device = self
            .get_selected_device()
            .expect("This crash is expected if discovered_devices is empty");

        debug!("(C) Pausing BLE scan");
        self.ble_scan_paused.store(true, Ordering::SeqCst);

        let device = Arc::new(selected_device.clone());
        let app_tx_clone = self.ble_tx.clone();

        debug!("Spawning characteristics thread");
        self.state = AppState::ConnectingForCharacteristics;
        tokio::spawn(async move { get_characteristics(app_tx_clone, device).await });
    }

    pub async fn connect_for_hr(&mut self, quick_connect_device: Option<&DeviceInfo>) {
        let selected_device = if let Some(device) = quick_connect_device {
            self.state = AppState::ConnectingForHeartRate;
            device
        } else {
            if self.discovered_devices.is_empty() {
                return;
            }
            // Let's check if we're okay asking to saving this device
            if !self.settings.ble.never_ask_to_save && self.state != AppState::SaveDevicePrompt {
                debug!("Asking to save device");
                self.state = AppState::SaveDevicePrompt;
                return;
            }

            self.state = AppState::ConnectingForHeartRate;
            self.get_selected_device().unwrap()
        };

        debug!("(HR) Pausing BLE scan");
        self.ble_scan_paused.store(true, Ordering::SeqCst);

        let device = Arc::new(selected_device.clone());
        let hr_tx_clone = self.hr_tx.clone();
        let shutdown_requested_clone = self.shutdown_requested.clone();
        // Not leaving as Duration as it's being used to check an abs difference
        let rr_twitch_threshold =
            Duration::from_millis(self.settings.osc.twitch_rr_threshold_ms as u64).as_secs_f32();
        let rr_ignore_after_empty = self.settings.ble.rr_ignore_after_empty as usize;
        debug!("Spawning notification thread, AppState: {:?}", self.state);
        self.hr_thread_handle = Some(tokio::spawn(async move {
            start_notification_thread(
                hr_tx_clone,
                device,
                rr_ignore_after_empty,
                rr_twitch_threshold,
                shutdown_requested_clone,
            )
            .await
        }));
    }

    pub async fn start_osc_thread(&mut self) {
        let osc_rx_clone = Arc::clone(&self.osc_rx);
        let osc_settings = self.settings.osc.clone();
        let shutdown_requested_clone = self.shutdown_requested.clone();

        debug!("Spawning OSC thread");
        self.osc_thread_handle = Some(tokio::spawn(async move {
            osc_thread(osc_rx_clone, osc_settings, shutdown_requested_clone).await
        }));
    }

    pub async fn start_logging_thread(&mut self) {
        let logging_rx_clone = Arc::clone(&self.log_rx);
        let misc_settings_clone = self.settings.misc.clone();
        let shutdown_requested_clone = self.shutdown_requested.clone();

        debug!("Spawning Data Logging thread");
        self.logging_thread_handle = Some(tokio::spawn(async move {
            logging_thread(
                logging_rx_clone,
                misc_settings_clone,
                shutdown_requested_clone,
            )
            .await
        }));
    }

    pub async fn start_dummy_thread(&mut self) {
        let hr_tx_clone = self.hr_tx.clone();
        let shutdown_requested_clone = self.shutdown_requested.clone();
        let dummy_settings_clone = self.settings.dummy.clone();
        debug!("Spawning Dummy thread");
        self.state = AppState::HeartRateView;
        self.chart_high_rr = self.settings.misc.session_chart_rr_max;
        self.hr_thread_handle = Some(tokio::spawn(async move {
            start_dummy_thread(hr_tx_clone, dummy_settings_clone, shutdown_requested_clone).await
        }));
    }

    pub async fn join_threads(&mut self) {
        let duration = Duration::from_secs(3);
        info!("Sending shutdown signal to threads!");
        self.shutdown_requested.cancel();

        if let Some(handle) = self.ble_thread_handle.take() {
            debug!("Joining BLE thread");
            if let Err(err) = timeout(duration, handle).await {
                error!("Failed to join BLE thread: {:?}", err);
            }
        }

        if let Some(handle) = self.hr_thread_handle.take() {
            debug!("Joining HR thread");
            if let Err(err) = timeout(duration, handle).await {
                error!("Failed to join HR thread: {:?}", err);
            }
        }

        if let Some(handle) = self.osc_thread_handle.take() {
            debug!("Joining OSC thread");
            if let Err(err) = timeout(duration, handle).await {
                error!("Failed to join OSC thread: {:?}", err);
            }
        }

        if let Some(handle) = self.logging_thread_handle.take() {
            debug!("Joining Logging thread");
            if let Err(err) = timeout(duration, handle).await {
                error!("Failed to join Logging thread: {:?}", err);
            }
        }

        if let Some(handle) = self.dummy_thread_handle.take() {
            debug!("Joining Dummy thread");
            if let Err(err) = timeout(duration, handle).await {
                error!("Failed to join Dummy thread: {:?}", err);
            }
        }
    }

    pub fn save_settings(&mut self) -> Result<(), std::io::Error> {
        self.settings.save()
    }

    pub fn try_save_device(&mut self, given_device: Option<&DeviceInfo>) {
        if self.allow_saving {
            let device = given_device.unwrap_or_else(|| self.get_selected_device().unwrap());

            let new_id = device.get_id();
            let new_name = device.name.clone();
            let mut damaged = false;
            if self.settings.ble.saved_address != new_id || self.settings.ble.saved_name != new_name
            {
                damaged = true;
            }
            // TODO See if I can find a way to get "Unknown" programatically,
            // not a fan of hardcoding it (and it's "" in the ::default())
            // Maybe do a .new() and supply a None?
            if damaged && new_name != "Unknown" {
                self.settings.ble.saved_address.clone_from(&new_id);
                self.settings.ble.saved_name.clone_from(&new_name);
                info!("Updating saved device! Name: {} MAC: {}", new_name, new_id);
                self.save_settings().expect("Failed to save settings");
            }
        }
    }

    pub fn get_selected_device(&self) -> Option<&DeviceInfo> {
        if let Some(selected_index) = self.table_state.selected() {
            self.discovered_devices.get(selected_index)
        } else {
            None
        }
    }

    pub fn is_idle_on_main_menu(&self) -> bool {
        self.error_message.is_none() && self.state == AppState::MainMenu
    }

    fn update_session_stats(&mut self, new_bpm: f64, new_rr: Option<&Duration>) {
        if self.session_low_bpm.0 == 0.0 || self.session_high_bpm.0 == 0.0 {
            self.chart_low_bpm = new_bpm - CHART_BPM_VERT_MARGIN;
            self.chart_high_bpm = new_bpm + CHART_BPM_VERT_MARGIN;
            self.session_low_bpm = (new_bpm, Local::now());
            self.session_high_bpm = (new_bpm, Local::now());
        } else if new_bpm > self.session_high_bpm.0 {
            self.session_high_bpm = (new_bpm, Local::now());
        } else if new_bpm < self.session_low_bpm.0 {
            self.session_low_bpm = (new_bpm, Local::now());
        }
        self.chart_high_bpm = self.chart_high_bpm.max(new_bpm);
        self.chart_low_bpm = self.chart_low_bpm.min(new_bpm);
        self.chart_mid_bpm = ((self.chart_low_bpm + self.chart_high_bpm) / 2.0).ceil();

        if let Some(rr) = new_rr {
            let rr_secs = rr.as_secs_f64();
            let rr_max = self.settings.misc.session_chart_rr_max;
            if self.chart_high_rr == 0.0 {
                self.chart_low_rr = (rr_secs - CHART_RR_VERT_MARGIN).max(rr_secs);
                self.chart_high_rr = (rr_secs + CHART_RR_VERT_MARGIN).min(rr_max);
            }
            if self.settings.misc.session_chart_rr_clamp_high && !self.settings.dummy.enabled {
                self.chart_high_rr = *self
                    .rr_history
                    .iter()
                    .reduce(|a, b| if a > b { a } else { b })
                    .unwrap_or(&0.0);
            } else {
                self.chart_high_rr = self.chart_high_rr.max(rr_secs);
            }
            if self.settings.misc.session_chart_rr_clamp_low {
                self.chart_high_rr = *self
                    .rr_history
                    .iter()
                    .reduce(|a, b| if a < b { a } else { b })
                    .unwrap_or(&0.0);
            } else {
                self.chart_low_rr = self.chart_low_rr.min(rr_secs);
            }

            self.chart_mid_rr = (self.chart_low_rr + self.chart_high_rr) / 2.0;
        }
    }

    fn update_chart_data(&mut self) {
        let bpm_enabled = self.settings.misc.session_chart_bpm_enabled;
        let rr_enabled = self.settings.misc.session_chart_rr_enabled;
        let combine = self.settings.misc.session_charts_combine;
        if rr_enabled {
            self.rr_dataset = self
                .rr_history
                .iter()
                .rev()
                .enumerate()
                .map(|(i, &x)| {
                    if bpm_enabled && combine {
                        let normalized =
                            (x - self.chart_low_rr) / (self.chart_high_rr - self.chart_low_rr);
                        let scaled = normalized * (self.chart_high_bpm - self.chart_low_bpm)
                            + self.chart_low_bpm;
                        (i as f64, scaled)
                    } else {
                        (i as f64, x)
                    }
                })
                .collect();
        }

        if bpm_enabled {
            self.bpm_dataset = self
                .heart_rate_history
                .iter()
                .rev()
                .enumerate()
                .map(|(i, &x)| (i as f64, x))
                .collect();
        }
    }

    pub fn append_to_history(&mut self, hr_data: &HeartRateStatus) {
        let bpm = hr_data.heart_rate_bpm as f64;
        let rr_max = self.settings.misc.session_chart_rr_max;
        if bpm > 0.0 {
            self.update_session_stats(bpm, hr_data.rr_intervals.last());

            self.heart_rate_history.push_back(bpm);
            if self.heart_rate_history.len() > CHART_BPM_MAX_ELEMENTS {
                self.heart_rate_history.pop_front();
            }
            for rr in &hr_data.rr_intervals {
                if rr.as_secs_f64() > rr_max {
                    continue;
                }
                self.rr_history.push_back(rr.as_secs_f64());
                if self.rr_history.len() > CHART_RR_MAX_ELEMENTS {
                    self.rr_history.pop_front();
                }
            }

            self.update_chart_data();
        }
    }

    fn table_state_scroll(up: bool, state: &mut TableState, table_len: usize) {
        if table_len == 0 {
            return;
        }
        let next = match state.selected() {
            Some(selected) => {
                if up {
                    (selected + table_len - 1) % table_len
                } else {
                    (selected + 1) % table_len
                }
            }
            None => 0,
        };
        state.select(Some(next));
    }

    pub fn scroll_down(&mut self) {
        match self.state {
            AppState::CharacteristicView => {
                self.characteristic_scroll = self.characteristic_scroll.wrapping_add(1);
            }
            AppState::MainMenu => {
                Self::table_state_scroll(
                    false,
                    &mut self.table_state,
                    self.discovered_devices.len(),
                );
            }
            AppState::SaveDevicePrompt => {
                Self::table_state_scroll(false, &mut self.save_prompt_state, 3);
            }
            _ => {}
        }
    }

    pub fn scroll_up(&mut self) {
        match self.state {
            AppState::CharacteristicView => {
                self.characteristic_scroll = self.characteristic_scroll.saturating_sub(1);
            }
            AppState::MainMenu => {
                Self::table_state_scroll(
                    true,
                    &mut self.table_state,
                    self.discovered_devices.len(),
                );
            }
            AppState::SaveDevicePrompt => {
                Self::table_state_scroll(true, &mut self.save_prompt_state, 3);
            }
            _ => {}
        }
    }
}
