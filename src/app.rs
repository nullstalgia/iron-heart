use chrono::{DateTime, Local};
use log::*;
use ratatui::widgets::TableState;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{
    broadcast::{self, Receiver as BReceiver, Sender as BSender},
    mpsc::{self, Receiver, Sender},
};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::heart_rate::ble::HEART_RATE_SERVICE_UUID;
use crate::heart_rate::dummy::dummy_thread;
use crate::heart_rate::websocket::websocket_thread;
use crate::widgets::save_prompt::SavePromptChoice;
use crate::{broadcast, ArgConfig};
use crate::{
    heart_rate::ble::start_notification_thread,
    heart_rate::HeartRateStatus,
    logging::file_logging_thread,
    osc::osc_thread,
    scan::{bluetooth_event_thread, get_characteristics},
    settings::Settings,
    structs::{Characteristic, DeviceInfo},
    widgets::heart_rate_display::{
        CHART_BPM_MAX_ELEMENTS, CHART_BPM_VERT_MARGIN, CHART_RR_MAX_ELEMENTS, CHART_RR_VERT_MARGIN,
    },
};

pub enum DeviceUpdate {
    ConnectedEvent(String),
    DisconnectedEvent(String),
    DeviceInfo(DeviceInfo),
    Characteristics(Vec<Characteristic>),
    Error(ErrorPopup),
}

#[derive(Debug, Clone)]
pub enum AppUpdate {
    HeartRateStatus(HeartRateStatus),
    WebsocketReady(std::net::SocketAddr),
    Error(ErrorPopup),
}

impl From<HeartRateStatus> for AppUpdate {
    fn from(hr: HeartRateStatus) -> Self {
        AppUpdate::HeartRateStatus(hr)
    }
}

impl From<ErrorPopup> for AppUpdate {
    fn from(error: ErrorPopup) -> Self {
        AppUpdate::Error(error)
    }
}

impl From<std::net::SocketAddr> for AppUpdate {
    fn from(addr: std::net::SocketAddr) -> Self {
        AppUpdate::WebsocketReady(addr)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AppState {
    MainMenu,
    CharacteristicView,
    SaveDevicePrompt,
    ConnectingForHeartRate,
    ConnectingForCharacteristics,
    WaitingForWebsocket,
    HeartRateView,
    HeartRateViewNoData,
}

#[derive(Debug, Clone)]
pub enum ErrorPopup {
    Intermittent(String),
    UserMustDismiss(String),
    Fatal(String),
}

pub struct App {
    // Devices as found by the BLE thread
    pub ble_rx: Receiver<DeviceUpdate>,
    pub ble_tx: Sender<DeviceUpdate>,
    // (Usually) Status updates from the heart rate monitor
    // Can also be errors from other actors
    pub broadcast_rx: BReceiver<AppUpdate>,
    pub broadcast_tx: BSender<AppUpdate>,
    pub error_message: Option<ErrorPopup>,
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
    pub settings: Settings,
    pub heart_rate_status: HeartRateStatus,
    pub cancel_app: CancellationToken,
    pub cancel_actors: CancellationToken,
    pub ble_thread_handle: Option<JoinHandle<()>>,
    pub hr_thread_handle: Option<JoinHandle<()>>,
    pub osc_thread_handle: Option<JoinHandle<()>>,
    pub logging_thread_handle: Option<JoinHandle<()>>,
    pub dummy_thread_handle: Option<JoinHandle<()>>,
    pub websocket_thread_handle: Option<JoinHandle<()>>,
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
    pub websocket_url: Option<String>,
    pub config_path: PathBuf,
}

impl App {
    pub fn build(arg_config: ArgConfig) -> Self {
        let (ble_tx, ble_rx) = mpsc::channel(50);
        let (broadcast_tx, broadcast_rx) = broadcast::channel::<AppUpdate>(50);

        let mut error_message = None;

        let exe_path = std::env::current_exe().expect("Failed to get executable path");

        let config_path = arg_config.config_override.unwrap_or_else(|| {
            let config_name = exe_path.with_extension("toml");
            let config_name = config_name
                .file_name()
                .expect("Failed to build config name");
            PathBuf::from(config_name)
        });

        let mut table_state = TableState::default();
        let mut save_prompt_state = TableState::default();
        table_state.select(Some(0));
        save_prompt_state.select(Some(0));

        let cancel_app = CancellationToken::new();
        let cancel_actors = cancel_app.child_token();

        let settings = Settings::load(config_path.clone()).unwrap_or_else(|err| {
            error!("Failed to load settings: {}", err);
            error_message = Some(ErrorPopup::Fatal(
                "Failed to load settings! Please fix file or delete to regenerate.".to_string(),
            ));
            Settings::default()
        });
        Self {
            ble_tx,
            ble_rx,
            broadcast_rx,
            broadcast_tx,
            ble_scan_paused: Arc::new(AtomicBool::default()),
            state: AppState::MainMenu,
            table_state,
            save_prompt_state,
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
            cancel_app,
            cancel_actors,
            ble_thread_handle: None,
            hr_thread_handle: None,
            osc_thread_handle: None,
            logging_thread_handle: None,
            dummy_thread_handle: None,
            websocket_thread_handle: None,
            session_high_bpm: (0.0, Local::now()),
            session_low_bpm: (0.0, Local::now()),
            chart_high_bpm: 0.0,
            chart_low_bpm: 0.0,
            chart_mid_bpm: 0.0,
            chart_high_rr: 0.0,
            chart_low_rr: 0.0,
            chart_mid_rr: 0.0,
            websocket_url: None,
            config_path,
        }
    }

    pub fn init(&mut self) {
        if let Some(error) = self.error_message.take() {
            self.handle_error_update(error);
            return;
        }
        if self.settings.save(&self.config_path).is_ok() {
            self.start_logging_thread();
            // HR source selection
            if self.settings.dummy.enabled {
                self.start_dummy_thread();
            } else if self.settings.websocket.enabled {
                self.start_websocket_thread();
            } else {
                self.start_bluetooth_event_thread();
            }

            if self.settings.osc.enabled {
                self.start_osc_thread();
            }
        }
    }

    pub async fn main_loop(&mut self) {
        // Check for updates from BLE Thread
        if let Ok(new_device_info) = self.ble_rx.try_recv() {
            self.device_info_callback(new_device_info)
        }

        // HR Notification Updates
        if let Ok(hr_data) = self.broadcast_rx.try_recv() {
            match hr_data {
                AppUpdate::HeartRateStatus(data) => {
                    // Assume we have proper data now
                    self.state = AppState::HeartRateView;
                    // Dismiss intermittent errors if we just got a notification packet
                    if let Some(ErrorPopup::Intermittent(_)) = self.error_message {
                        self.error_message = None;
                    }
                    self.append_to_history(&data);
                    self.heart_rate_status = data;
                }
                AppUpdate::Error(error) => self.handle_error_update(error),
                AppUpdate::WebsocketReady(local_addr) => {
                    self.websocket_url = Some(local_addr.to_string());
                }
            }
        }
    }

    // TODO Proper actor/handle structures for threads
    // This is a bit much
    pub fn start_bluetooth_event_thread(&mut self) {
        let pause_signal_clone = Arc::clone(&self.ble_scan_paused);
        let app_tx_clone = self.ble_tx.clone();
        let shutdown_requested_clone = self.cancel_actors.clone();
        debug!("Spawning Bluetooth CentralEvent thread");
        self.ble_thread_handle = Some(tokio::spawn(async move {
            bluetooth_event_thread(app_tx_clone, pause_signal_clone, shutdown_requested_clone).await
        }));
    }

    pub fn connect_for_characteristics(&mut self) {
        let Some(selected_device) = self.get_selected_device() else {
            return;
        };

        debug!("(C) Pausing BLE scan");
        self.ble_scan_paused.store(true, Ordering::SeqCst);

        let device = selected_device.clone();
        let app_tx_clone = self.ble_tx.clone();

        debug!("Spawning characteristics thread");
        self.state = AppState::ConnectingForCharacteristics;
        // TODO make this not another thread maybe
        tokio::spawn(async move { get_characteristics(app_tx_clone, device).await });
    }

    pub fn connect_for_hr(&mut self, quick_connect_device: Option<&DeviceInfo>) {
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

        let device = selected_device.clone();
        let hr_tx_clone = self.broadcast_tx.clone();
        let shutdown_requested_clone = self.cancel_actors.clone();
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

    pub fn start_osc_thread(&mut self) {
        let osc_settings = self.settings.osc.clone();
        let broadcast_rx = self.broadcast_tx.subscribe();
        let broadcast_tx = self.broadcast_tx.clone();
        let shutdown_requested_clone = self.cancel_actors.clone();

        debug!("Spawning OSC thread");
        self.osc_thread_handle = Some(tokio::spawn(async move {
            osc_thread(
                broadcast_rx,
                broadcast_tx,
                osc_settings,
                shutdown_requested_clone,
            )
            .await
        }));
    }

    pub fn start_logging_thread(&mut self) {
        let misc_settings_clone = self.settings.misc.clone();
        let shutdown_requested_clone = self.cancel_actors.clone();
        let broadcast_rx = self.broadcast_tx.subscribe();
        let broadcast_tx = self.broadcast_tx.clone();

        debug!("Spawning Data Logging thread");
        self.logging_thread_handle = Some(tokio::spawn(async move {
            file_logging_thread(
                broadcast_rx,
                broadcast_tx,
                misc_settings_clone,
                shutdown_requested_clone,
            )
            .await
        }));
    }

    pub fn start_dummy_thread(&mut self) {
        let broadcast_tx = self.broadcast_tx.clone();
        let shutdown_requested_clone = self.cancel_actors.clone();
        let dummy_settings_clone = self.settings.dummy.clone();
        debug!("Spawning Dummy thread");
        self.state = AppState::HeartRateView;
        self.chart_high_rr = self.settings.misc.chart_rr_max;
        self.dummy_thread_handle = Some(tokio::spawn(async move {
            dummy_thread(broadcast_tx, dummy_settings_clone, shutdown_requested_clone).await
        }));
    }

    pub fn start_websocket_thread(&mut self) {
        let broadcast_tx = self.broadcast_tx.clone();
        let shutdown_requested_clone = self.cancel_actors.clone();
        let websocket_settings_clone = self.settings.websocket.clone();
        debug!("Spawning Websocket thread");
        self.state = AppState::WaitingForWebsocket;
        self.websocket_thread_handle = Some(tokio::spawn(async move {
            websocket_thread(
                broadcast_tx,
                websocket_settings_clone,
                shutdown_requested_clone,
            )
            .await
        }));
    }

    pub async fn join_threads(&mut self) {
        let duration = Duration::from_secs(3);
        info!("Sending shutdown signal to threads!");
        self.cancel_app.cancel();

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

        if let Some(handle) = self.websocket_thread_handle.take() {
            debug!("Joining Websocket thread");
            if let Err(err) = timeout(duration, handle).await {
                error!("Failed to join Websocket thread: {:?}", err);
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

    pub fn try_save_settings(&mut self) {
        self.settings.save(&self.config_path).unwrap_or_else(|e| {
            self.handle_error_update(ErrorPopup::Fatal(format!("Couldn't save settings! {e}")))
        });
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
                self.try_save_settings();
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
            let rr_max = self.settings.misc.chart_rr_max;
            if self.chart_high_rr == 0.0 {
                self.chart_low_rr = (rr_secs - CHART_RR_VERT_MARGIN).max(rr_secs);
                self.chart_high_rr = (rr_secs + CHART_RR_VERT_MARGIN).min(rr_max);
            }
            if self.settings.misc.chart_rr_clamp_high && !self.settings.dummy.enabled {
                self.chart_high_rr = *self
                    .rr_history
                    .iter()
                    .reduce(|a, b| if a > b { a } else { b })
                    .unwrap_or(&0.0);
            } else {
                self.chart_high_rr = self.chart_high_rr.max(rr_secs);
            }
            if self.settings.misc.chart_rr_clamp_low {
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
        let bpm_enabled = self.settings.misc.chart_bpm_enabled;
        let rr_enabled = self.settings.misc.chart_rr_enabled;
        let combine = self.settings.misc.charts_combine;
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
        let rr_max = self.settings.misc.chart_rr_max;
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

    fn handle_error_update(&mut self, error: ErrorPopup) {
        // Don't override a fatal error popup
        if matches!(self.error_message, Some(ErrorPopup::Fatal(_))) {
            return;
        }
        match error {
            ErrorPopup::Fatal(e) => {
                self.error_message = Some(ErrorPopup::Fatal(e));
                // Tell actors to stop, but let user close UI
                self.cancel_actors.cancel();
                // Just for the UI, "stop" the scan
                self.ble_scan_paused.store(true, Ordering::SeqCst);
            }
            _ => self.error_message = Some(error),
        }
    }

    /// Terminal interval tick
    pub fn term_tick(&mut self) {
        self.frame_count = self.frame_count.checked_add(1).unwrap_or(0);
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
    pub fn enter_pressed(&mut self) {
        // Dismiss error message if present
        if self.error_message.is_some() {
            match self.error_message.as_ref().unwrap() {
                ErrorPopup::UserMustDismiss(_) | ErrorPopup::Intermittent(_) => {
                    self.error_message = None;
                }
                ErrorPopup::Fatal(_) => {
                    self.cancel_app.cancel();
                }
            }
            // Skip other checks if we dismissed an error.
            return;
        }

        match self.state {
            AppState::CharacteristicView => self.state = AppState::MainMenu,
            AppState::SaveDevicePrompt => {
                let chosen_option = self.save_prompt_state.selected().unwrap_or(0);
                match SavePromptChoice::from(chosen_option) {
                    SavePromptChoice::Yes => {
                        self.allow_saving = true;
                        self.try_save_settings();
                    }
                    SavePromptChoice::No => {}
                    SavePromptChoice::Never => {
                        self.settings.ble.never_ask_to_save = true;
                        self.try_save_settings();
                    }
                }
                debug!(
                    "Connecting from save prompt | Chosen option: {}",
                    chosen_option
                );
                self.connect_for_hr(None);
            }
            AppState::MainMenu => {
                // app_state changed by method
                debug!("Connecting from main menu");
                self.connect_for_hr(None);
            }
            _ => {}
        }
    }

    /// Callback to handle new/updated devices found by the BLE scan thread
    pub fn device_info_callback(&mut self, new_device_info: DeviceUpdate) {
        match new_device_info {
            DeviceUpdate::DeviceInfo(device) => {
                // If the device is already in the list, update it
                if let Some(existing_device) = self
                    .discovered_devices
                    .iter_mut()
                    .find(|d| d.id == device.id)
                {
                    *existing_device = device.clone();
                    //self.discovered_devices[existing_device_index] = device.clone();
                } else {
                    // If the device is not in the list, add it
                    // but only if it has the heart rate service
                    // (We don't use the ScanFilter from btleplug to allow quicker connection to saved devices,
                    // and since it reports only "Unknown" names for some reason)
                    // TODO: Raise issue about it
                    if device
                        .services
                        .iter()
                        .any(|service| *service == HEART_RATE_SERVICE_UUID)
                    {
                        self.discovered_devices.push(device.clone());
                    }
                    // This filter used to be in scan.rs, but doing it here
                    // lets us connect to saved devices without checking their services (i.e. quicker)
                }

                // If the device is saved, connect to it
                if (device.id == self.settings.ble.saved_address
                    || device.name == self.settings.ble.saved_name)
                    && self.is_idle_on_main_menu()
                {
                    self.quick_connect_ui = true;
                    // I'm going to assume that if we find a set saved device,
                    // they're always going to want to update the value in case Name/MAC changes,
                    // even if they're weird and have set `never_ask_to_save` to true
                    self.allow_saving = true;
                    // Adding device to UI list so other parts of the app that check the selected device
                    // get the expected result
                    if !self.discovered_devices.iter().any(|d| d.id == device.id) {
                        self.discovered_devices.push(device.clone());
                    }
                    self.table_state.select(
                        self.discovered_devices
                            .iter()
                            .position(|d| d.id == device.id),
                    );
                    self.try_save_device(Some(&device));
                    debug!("Connecting to saved device, AppState: {:?}", self.state);
                    // app_state changed by method
                    self.connect_for_hr(Some(&device));
                } else {
                    self.try_save_device(None);
                }
            }
            DeviceUpdate::Characteristics(characteristics) => {
                self.selected_characteristics = characteristics;
                self.state = AppState::CharacteristicView
            }
            DeviceUpdate::Error(error) => {
                error!("BLE Thread Error: {:?}", error.clone());
                if self.state == AppState::HeartRateViewNoData
                    && matches!(error, ErrorPopup::Intermittent(_))
                {
                    // Ignoring the intermittent ones when we're in the inbetween state
                } else {
                    // Don't override a fatal error
                    if !matches!(self.error_message, Some(ErrorPopup::Fatal(_))) {
                        self.error_message = Some(error);
                    }
                }
                if self.state == AppState::HeartRateView
                    || self.state == AppState::HeartRateViewNoData
                    || self.state == AppState::ConnectingForHeartRate
                {
                    broadcast!(
                        self.broadcast_tx,
                        HeartRateStatus::default(),
                        "Failed to send 0BPM on BLE Error"
                    );
                }
                //self.is_loading_characteristics = false;
            }
            DeviceUpdate::ConnectedEvent(id) => {
                info!("Connnnnnnnn");
                if self.state == AppState::ConnectingForCharacteristics {
                    self.state = AppState::CharacteristicView;
                } else {
                    self.state = if self.heart_rate_status.heart_rate_bpm > 0 {
                        AppState::HeartRateView
                    } else {
                        AppState::HeartRateViewNoData
                    };
                }

                if self.state == AppState::HeartRateView
                    || self.state == AppState::HeartRateViewNoData
                    || self.state == AppState::ConnectingForHeartRate
                {
                    if id == self.get_selected_device().unwrap().id {
                        debug!("Connected to device {:?}, stopping BLE scan", id);
                        self.ble_scan_paused.store(true, Ordering::SeqCst);
                    }
                    self.try_save_device(None);
                }
            }
            DeviceUpdate::DisconnectedEvent(id) => {
                self.error_message = Some(ErrorPopup::Intermittent(
                    "Disconnected from device!".to_string(),
                ));
                if (self.state == AppState::HeartRateView
                    || self.state == AppState::HeartRateViewNoData
                    || self.state == AppState::MainMenu)
                    && id == self.get_selected_device().unwrap().id
                {
                    debug!("Disconnected from device {:?}, resuming BLE scan", id);
                    broadcast!(
                        self.broadcast_tx,
                        HeartRateStatus::default(),
                        "Failed to send 0BPM on BLE DC"
                    );
                    self.ble_scan_paused.store(false, Ordering::SeqCst);
                }
            }
        }

        if self.table_state.selected().is_none() {
            self.table_state.select(Some(0));
        }
    }
}
