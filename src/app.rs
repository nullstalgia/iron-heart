use chrono::{DateTime, Local};
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
use tracing::{debug, error, info};

use crate::activities::Activities;
use crate::args::{SubCommands, TopLevelCmd};
use crate::broadcast;
use crate::errors::AppError;
use crate::heart_rate::ble::HEART_RATE_SERVICE_UUID;
use crate::heart_rate::dummy::dummy_thread;
use crate::heart_rate::websocket::websocket_thread;
use crate::logging::prometheus_logging_thread;
use crate::ui::table_state_scroll;
use crate::updates::{UpdateHandle, UpdateReply};
use crate::vrcx::VrcxStartup;
use crate::widgets::prompts::SavePromptChoice;
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

pub enum AppRx {
    DeviceUpdate(DeviceUpdate),
    AppUpdate(AppUpdate),
    UpdateReply(UpdateReply),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
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
    ActivitySelected(u8),
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
pub enum AppView {
    BleDeviceSelection,
    WaitingForWebsocket,
    HeartRateView,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub enum SubState {
    #[default]
    None,
    #[cfg(windows)]
    VrcxAutostartPrompt,
    ConnectingForCharacteristics,
    CharacteristicView,
    SaveDevicePrompt,
    ConnectingForHeartRate,
    ActivitySelection,
    ActivityCreation,
    UpdateAllowCheckPrompt,
    UpdateFoundPrompt,
    UpdateDownloading,
    #[cfg(windows)]
    LaunchUpdatePrompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorPopup {
    Intermittent(String),
    UserMustDismiss(String),
    Fatal(String),
    FatalDetailed(String, String),
}

impl ErrorPopup {
    pub fn detailed(message: &str, error: AppError) -> Self {
        Self::FatalDetailed(message.to_owned(), error.to_string())
    }
    // pub fn detailed_str(message: &str, error: &str) -> Self {
    //     Self::FatalDetailed(message.to_owned(), error.to_owned())
    // }
}

pub struct App {
    // Devices as found by the BLE thread
    pub ble_rx: Receiver<DeviceUpdate>,
    pub ble_tx: Sender<DeviceUpdate>,
    // A Sender that can be used to trigger the BLE thread to restart it's objects from other threads
    ble_restart_tx: Option<Sender<()>>,
    // (Usually) Status updates from the heart rate monitor
    // Can also be errors from other actors
    pub broadcast_rx: BReceiver<AppUpdate>,
    pub broadcast_tx: BSender<AppUpdate>,
    pub error_message: Option<ErrorPopup>,
    pub ble_scan_paused: Arc<AtomicBool>,
    pub view: AppView,
    pub sub_state: SubState,
    pub table_state: TableState,
    pub prompt_state: TableState,
    pub should_save_ble_device: bool,
    pub allow_modifying_config: bool,
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
    pub file_logging_handle: Option<JoinHandle<()>>,
    pub prometheus_handle: Option<JoinHandle<()>>,
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
    ignore_margins_for_vhs: bool,
    pub websocket_url: Option<String>,
    pub config_path: PathBuf,
    vrcx: VrcxStartup,
    pub activities: Activities,
    pub updates: UpdateHandle,
    pub update_download_percentage: f64,
    pub update_newer_version: Option<String>,
}

impl App {
    pub fn build(arg_config: &TopLevelCmd, parent_token: Option<CancellationToken>) -> Self {
        let (ble_tx, ble_rx) = mpsc::channel(50);
        let (broadcast_tx, broadcast_rx) = broadcast::channel::<AppUpdate>(50);

        let mut error_message = None;

        let exe_path = std::env::current_exe().expect("Failed to get executable path");

        let config_path: PathBuf = match arg_config.config_override.as_ref() {
            Some(path) => path.to_owned(),
            None => {
                let config_name = exe_path.with_extension("toml");
                let config_name = config_name
                    .file_name()
                    .expect("Failed to build config name");
                PathBuf::from(config_name)
            }
        };

        let mut table_state = TableState::default();
        let mut prompt_state = TableState::default();
        table_state.select(Some(0));
        prompt_state.select(Some(0));

        let cancel_app = parent_token.unwrap_or_default();
        let cancel_actors = cancel_app.child_token();

        let allow_modifying_config = !arg_config.no_save;
        let settings = match Settings::load(config_path.clone(), arg_config.config_required) {
            Ok(settings) => settings,
            Err(e) => {
                error!("Failed to load settings: {}", e);
                error_message = Some(ErrorPopup::detailed(
                    "Failed to load settings! Please fix file or delete to regenerate.",
                    e,
                ));
                Settings::default()
            }
        };
        Self {
            ble_tx,
            ble_rx,
            ble_restart_tx: None,
            broadcast_rx,
            broadcast_tx,
            ble_scan_paused: Arc::new(AtomicBool::default()),
            view: AppView::BleDeviceSelection,
            sub_state: SubState::None,
            table_state,
            prompt_state,
            should_save_ble_device: false,
            allow_modifying_config,
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
            file_logging_handle: None,
            prometheus_handle: None,
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
            ignore_margins_for_vhs: false,
            websocket_url: None,
            config_path,
            vrcx: VrcxStartup::new(),
            activities: Activities::new(),
            updates: UpdateHandle::new(),
            update_download_percentage: 0.0,
            update_newer_version: None,
        }
    }

    pub async fn init(&mut self, arg_config: &TopLevelCmd) {
        // Return early if error is present
        if let Some(error) = self.error_message.take() {
            self.handle_error_update(error);
            return;
        }
        // Or if initial config save failed
        if !self.try_save_settings() {
            return;
        }
        let Some(activity) = self.try_load_activities().await else {
            return;
        };
        // self.handle_error_update(ErrorPopup::Fatal(format!("{:?}", self.activities)));
        // return;
        if self.settings.osc.enabled {
            self.start_osc_thread(activity);
        }
        self.start_logging_threads(activity.unwrap_or(0));
        // HR source selection
        if let Some(subcommands) = arg_config.subcommands.as_ref() {
            match subcommands {
                SubCommands::Ble(_) => self.start_bluetooth_event_thread(),
                SubCommands::Dummy(dummy) => {
                    self.ignore_margins_for_vhs = dummy.vhs;
                    self.start_dummy_thread(dummy.speed, dummy.vhs);
                }
                SubCommands::WebSocket(ws) => self.start_websocket_thread(ws.port),
            }
            return;
        }

        if self.settings.dummy.enabled {
            self.start_dummy_thread(None, false);
        } else if self.settings.websocket.enabled {
            self.start_websocket_thread(None);
        } else {
            self.start_bluetooth_event_thread();
        }
    }

    /// Returns None if activities didn't load properly (error handling is handled in here)
    ///
    /// Returns Some(None) if activities were disabled.
    ///
    /// Returns Some(Some(index)) of this sessions initial activity
    async fn try_load_activities(&mut self) -> Option<Option<u8>> {
        // Don't bother loading, return a success
        if !self.settings.activities.enabled {
            return Some(None);
        }

        if let Err(e) = self
            .activities
            .load(self.settings.activities.remember_last)
            .await
        {
            self.handle_error_update(ErrorPopup::detailed("Couldn't load activities!", e));

            None
        } else {
            Some(Some(self.activities.current_activity))
        }
    }

    // Had to break this apart into two functions, since the parent
    // tokio::select could cancel any concurrent handling if a terminal event came in
    pub async fn app_receivers(&mut self) -> AppRx {
        tokio::select! {
            // Check for updates from BLE Thread
            Some(new_device_info) = self.ble_rx.recv() => {
                // debug!("ble: {new_device_info:?}");
                AppRx::DeviceUpdate(new_device_info)
            }
            // HR Notification Updates
            Ok(hr_data) = self.broadcast_rx.recv() => {
                // debug!("broadcast: {hr_data:?}");
                AppRx::AppUpdate(hr_data)
            }
            // Replies from the executable self-updating task
            Some(data) = self.updates.reply_rx.recv() => {
                // debug!("update: {data:?}");
                AppRx::UpdateReply(data)
            }
        }
    }

    pub async fn app_handlers(&mut self, data: AppRx) {
        match data {
            AppRx::DeviceUpdate(new_device_info) => self.device_info_callback(new_device_info),
            AppRx::AppUpdate(hr_data) => {
                match hr_data {
                    AppUpdate::HeartRateStatus(data) => {
                        if data.heart_rate_bpm > 0 || !data.rr_intervals.is_empty() {
                            // Assume we have proper data now
                            self.view = AppView::HeartRateView;
                            if self.sub_state == SubState::ConnectingForHeartRate {
                                self.sub_state = SubState::None;
                            }
                            // Dismiss intermittent errors if we just got a notification packet
                            if let Some(ErrorPopup::Intermittent(_)) = self.error_message {
                                self.error_message = None;
                            }
                            self.append_to_history(&data);
                        }
                        self.heart_rate_status = data;
                    }
                    AppUpdate::Error(error) => self.handle_error_update(error),
                    AppUpdate::WebsocketReady(local_addr) => {
                        self.websocket_url = Some(local_addr.to_string());
                    }
                    AppUpdate::ActivitySelected(_) => {
                        if let Err(err) = self.activities.save().await {
                            self.handle_error_update(ErrorPopup::detailed(
                                "Failed to save activities!",
                                err,
                            ));
                        }
                    }
                }
            }
            AppRx::UpdateReply(data) => match data {
                UpdateReply::UpToDate => {
                    info!("App is Up to Date!");
                }
                UpdateReply::UpdateFound(version) => {
                    info!("Newer Version Found: {version}");
                    if self.settings.updates.version_skipped.eq(&version) {
                        info!("Version marked as skipped!");
                        self.updates.reply_rx.close();
                        return;
                    }
                    self.update_newer_version = Some(version);
                    self.prompt_state.select(Some(1));
                    self.sub_state = SubState::UpdateFoundPrompt;
                }
                UpdateReply::DownloadProgress(percentage) => {
                    self.update_download_percentage = percentage;
                }
                #[cfg(windows)]
                UpdateReply::ReadyToLaunch => {
                    self.prompt_state.select(Some(0));
                    self.sub_state = SubState::LaunchUpdatePrompt;
                }
                #[cfg(not(windows))]
                UpdateReply::ReadyToLaunch => {
                    self.updates.start_new_version();
                }
                UpdateReply::Error(err) => {
                    self.handle_error_update(ErrorPopup::detailed(
                        "Error during auto update:",
                        err,
                    ));
                }
            },
        }
    }

    pub async fn first_time_setup(&mut self, arg_config: &TopLevelCmd) {
        // Return early if init() had an issue/is a Dummy right now
        // (Not using is_idle_on_ble since I want this to work for WS and BLE users)
        if self.error_message.is_some()
            || self.sub_state != SubState::None
            || self.view == AppView::HeartRateView
        {
            return;
        }
        if !self.allow_modifying_config {
            return;
        }
        if arg_config.skip_prompts {
            return;
        }

        #[cfg(windows)]
        if let Err(e) = self.vrcx.init().await {
            self.handle_error_update(ErrorPopup::Intermittent(format!(
                "VRCX Shortcut Error: {e}"
            )));
            return;
        }

        if self.vrcx.vrcx_installed() && !self.vrcx.shortcut_exists() {
            self.vrcx_prompt();
        } else {
            self.auto_update_prompt();
        }
    }

    #[cfg(windows)]
    fn vrcx_prompt(&mut self) {
        if self.settings.misc.vrcx_shortcut_prompt {
            self.prompt_state.select(Some(0));
            self.sub_state = SubState::VrcxAutostartPrompt;
        } else {
            self.auto_update_prompt();
        }
    }

    #[cfg(not(windows))]
    fn vrcx_prompt(&mut self) {
        // When not on windows, just skip to auto-update prompt
        self.auto_update_prompt();
    }

    pub fn auto_update_prompt(&mut self) {
        self.sub_state = SubState::None;

        // In case the user set updates true externally without also changing prompt
        if self.settings.updates.allow_checking_for_updates {
            self.spawn_update_check();
        } else if self.settings.updates.update_check_prompt {
            // But if we haven't asked the user yet, do that first.
            self.prompt_state.select(Some(0));
            self.sub_state = SubState::UpdateAllowCheckPrompt;
        }
    }

    fn spawn_update_check(&mut self) {
        self.updates.query_latest();
    }

    // TODO Proper actor/handle structures for threads
    // This is a bit much
    pub fn start_bluetooth_event_thread(&mut self) {
        let pause_signal_clone = Arc::clone(&self.ble_scan_paused);
        let app_tx_clone = self.ble_tx.clone();
        let shutdown_requested_clone = self.cancel_actors.clone();
        let (restart_tx, restart_rx) = mpsc::channel(1);
        self.ble_restart_tx = Some(restart_tx);
        debug!("Spawning Bluetooth CentralEvent thread");
        self.ble_thread_handle = Some(tokio::spawn(async move {
            bluetooth_event_thread(
                app_tx_clone,
                restart_rx,
                pause_signal_clone,
                shutdown_requested_clone,
            )
            .await
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
        self.sub_state = SubState::ConnectingForCharacteristics;
        // TODO make this not another thread maybe
        tokio::spawn(async move { get_characteristics(app_tx_clone, device).await });
    }

    pub fn connect_for_hr(&mut self, quick_connect_device: Option<&DeviceInfo>) {
        if self.hr_thread_handle.is_some() {
            debug!("Not spawning extra notification thread");
            return;
        }
        let selected_device = if let Some(device) = quick_connect_device {
            device
        } else {
            if self.discovered_devices.is_empty() {
                return;
            }
            // Let's check if we're okay asking to saving this device
            if !self.settings.ble.never_ask_to_save
                && self.allow_modifying_config
                && self.sub_state != SubState::SaveDevicePrompt
                // Skip the prompt if we know the device
                && !self.is_device_saved(None)
            {
                debug!("Asking to save device");
                self.prompt_state.select(Some(0));
                self.sub_state = SubState::SaveDevicePrompt;
                return;
            }

            let selected_index = self.table_state.selected().unwrap_or(0);
            self.discovered_devices
                .get(selected_index)
                .expect("Chosen device missing")
        };

        debug!("(HR) Pausing BLE scan");
        self.ble_scan_paused.store(true, Ordering::SeqCst);
        self.sub_state = SubState::ConnectingForHeartRate;

        let device = selected_device.clone();
        let hr_tx_clone = self.broadcast_tx.clone();
        let restart_tx_clone = self.ble_restart_tx.clone().expect("BLE Restart TX missing");
        let shutdown_requested_clone = self.cancel_actors.clone();
        let ble_packet_timeout = self.settings.ble.packet_timeout_secs;
        let ble_packet_timeout = if ble_packet_timeout == 0 {
            Duration::from_secs(30)
        } else {
            Duration::from_secs(self.settings.ble.packet_timeout_secs as u64)
        };
        // Not leaving as Duration as it's being used to check an abs difference
        let rr_twitch_threshold =
            Duration::from_millis(self.settings.osc.twitch_rr_threshold_ms as u64).as_secs_f32();
        let rr_ignore_after_empty = self.settings.ble.rr_ignore_after_empty as usize;
        debug!("Spawning notification thread, AppView: {:?}", self.view);
        self.hr_thread_handle = Some(tokio::spawn(async move {
            start_notification_thread(
                hr_tx_clone,
                restart_tx_clone,
                device,
                rr_ignore_after_empty,
                rr_twitch_threshold,
                ble_packet_timeout,
                shutdown_requested_clone,
            )
            .await
        }));
    }

    fn is_device_saved(&self, given_device: Option<&DeviceInfo>) -> bool {
        if self.settings.ble.saved_name.is_empty() && self.settings.ble.saved_address.is_empty() {
            return false;
        }

        let device = given_device.unwrap_or_else(|| self.get_selected_device().unwrap());

        device.name == self.settings.ble.saved_name
            || device.address == self.settings.ble.saved_address
    }

    pub fn start_osc_thread(&mut self, initial_activity: Option<u8>) {
        let osc_settings = self.settings.osc.clone();
        let broadcast_rx = self.broadcast_tx.subscribe();
        let broadcast_tx = self.broadcast_tx.clone();
        let shutdown_requested_clone = self.cancel_actors.clone();

        debug!("Spawning OSC thread");
        self.osc_thread_handle = Some(tokio::spawn(async move {
            osc_thread(
                broadcast_rx,
                broadcast_tx,
                initial_activity,
                osc_settings,
                shutdown_requested_clone,
            )
            .await
        }));
    }

    pub fn start_logging_threads(&mut self, initial_activity: u8) {
        let file_logging_enabled = self.settings.misc.log_sessions_to_csv
            || self.settings.misc.write_bpm_to_file
            || self.settings.misc.write_rr_to_file;
        if file_logging_enabled {
            let misc_settings_clone = self.settings.misc.clone();
            let shutdown_requested_clone = self.cancel_actors.clone();
            let broadcast_rx = self.broadcast_tx.subscribe();
            let broadcast_tx = self.broadcast_tx.clone();

            debug!("Spawning Data Logging thread");
            self.file_logging_handle = Some(tokio::spawn(async move {
                file_logging_thread(
                    broadcast_rx,
                    broadcast_tx,
                    initial_activity,
                    misc_settings_clone,
                    shutdown_requested_clone,
                )
                .await
            }));
        }

        if self.settings.prometheus.enabled {
            let prometheus_settings_clone = self.settings.prometheus.clone();
            let shutdown_requested_clone = self.cancel_actors.clone();
            let broadcast_rx = self.broadcast_tx.subscribe();
            let broadcast_tx = self.broadcast_tx.clone();

            debug!("Spawning Prometheus thread");
            self.prometheus_handle = Some(tokio::spawn(async move {
                prometheus_logging_thread(
                    broadcast_rx,
                    broadcast_tx,
                    initial_activity,
                    prometheus_settings_clone,
                    shutdown_requested_clone,
                )
                .await
            }));
        }
    }

    pub fn start_dummy_thread(&mut self, seconds_override: Option<f32>, vhs_prefill: bool) {
        let broadcast_tx = self.broadcast_tx.clone();
        let shutdown_requested_clone = self.cancel_actors.clone();
        let dummy_settings_clone = self.settings.dummy.clone();
        debug!("Spawning Dummy thread");
        self.view = AppView::HeartRateView;
        self.chart_high_rr = self.settings.tui.chart_rr_max;
        self.dummy_thread_handle = Some(tokio::spawn(async move {
            dummy_thread(
                broadcast_tx,
                dummy_settings_clone,
                seconds_override,
                vhs_prefill,
                shutdown_requested_clone,
            )
            .await
        }));
    }

    pub fn start_websocket_thread(&mut self, port_override: Option<u16>) {
        let broadcast_tx = self.broadcast_tx.clone();
        let shutdown_requested_clone = self.cancel_actors.clone();
        let websocket_settings_clone = self.settings.websocket.clone();
        let ws_packet_timeout = self.settings.websocket.packet_timeout_secs;
        let ws_packet_timeout = if ws_packet_timeout == 0 {
            Duration::from_secs(30)
        } else {
            Duration::from_secs(self.settings.ble.packet_timeout_secs as u64)
        };
        // Not leaving as Duration as it's being used to check an abs difference
        let rr_twitch_threshold =
            Duration::from_millis(self.settings.osc.twitch_rr_threshold_ms as u64).as_secs_f32();
        debug!("Spawning Websocket thread");
        self.view = AppView::WaitingForWebsocket;
        self.websocket_thread_handle = Some(tokio::spawn(async move {
            websocket_thread(
                broadcast_tx,
                websocket_settings_clone,
                port_override,
                rr_twitch_threshold,
                ws_packet_timeout,
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

        if let Some(handle) = self.file_logging_handle.take() {
            debug!("Joining File Logging thread");
            if let Err(err) = timeout(duration, handle).await {
                error!("Failed to join File Logging thread: {:?}", err);
            }
        }

        if let Some(handle) = self.prometheus_handle.take() {
            debug!("Joining Prometheus thread");
            if let Err(err) = timeout(duration, handle).await {
                error!("Failed to join Prometheus thread: {:?}", err);
            }
        }

        if let Some(handle) = self.dummy_thread_handle.take() {
            debug!("Joining Dummy thread");
            if let Err(err) = timeout(duration, handle).await {
                error!("Failed to join Dummy thread: {:?}", err);
            }
        }
    }

    /// Wrapper for save_settings that handles errors and returns just a success bool
    pub fn try_save_settings(&mut self) -> bool {
        if let Err(e) = self.save_settings() {
            error!("Couldn't save settings! {}", e);
            self.handle_error_update(ErrorPopup::detailed("Couldn't save settings!", e));

            false
        } else {
            true
        }
    }

    fn save_settings(&mut self) -> Result<(), AppError> {
        if self.allow_modifying_config
        // && !self.cancel_actors.is_cancelled()
        {
            self.settings.save(&self.config_path)
        } else {
            Ok(())
        }
    }

    pub fn try_save_device(&mut self, given_device: Option<&DeviceInfo>) {
        if self.should_save_ble_device && self.allow_modifying_config
        // && !self.cancel_actors.is_cancelled()
        {
            let device = given_device.unwrap_or_else(|| self.get_selected_device().unwrap());

            let new_id = device.get_id();
            let new_name = device.name.clone();

            if new_id.is_empty() || new_name.is_empty() {
                return;
            }

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

    pub fn is_idle_on_ble_selection(&self) -> bool {
        self.error_message.is_none()
            && self.view == AppView::BleDeviceSelection
            && self.sub_state == SubState::None
    }

    fn datasets_empty(&self) -> bool {
        self.heart_rate_history.is_empty() && self.rr_history.is_empty()
    }

    fn update_session_stats(&mut self, new_bpm: f64, new_rr: Option<&Duration>) {
        if self.session_low_bpm.0 == 0.0 || self.session_high_bpm.0 == 0.0 {
            let margin = if self.ignore_margins_for_vhs {
                0.0
            } else {
                CHART_BPM_VERT_MARGIN
            };
            self.chart_low_bpm = new_bpm - margin;
            self.chart_high_bpm = new_bpm + margin;
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
            let rr_max = self.settings.tui.chart_rr_max;
            if self.chart_high_rr == 0.0 {
                self.chart_low_rr = (rr_secs - CHART_RR_VERT_MARGIN).max(rr_secs);
                self.chart_high_rr = (rr_secs + CHART_RR_VERT_MARGIN).min(rr_max);
            }
            if self.settings.tui.chart_rr_clamp_high && !self.settings.dummy.enabled {
                self.chart_high_rr = *self
                    .rr_history
                    .iter()
                    .reduce(|a, b| if a > b { a } else { b })
                    .unwrap_or(&0.0);
            } else {
                self.chart_high_rr = self.chart_high_rr.max(rr_secs);
            }
            if self.settings.tui.chart_rr_clamp_low {
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
        let bpm_enabled = self.settings.tui.chart_bpm_enabled;
        let rr_enabled = self.settings.tui.chart_rr_enabled;
        let combine = self.settings.tui.charts_combine;
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
        let rr_max = self.settings.tui.chart_rr_max;
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

    pub fn handle_error_update(&mut self, error: ErrorPopup) {
        // Never override a fatal error popup
        match self.error_message {
            Some(ErrorPopup::Fatal(_)) | Some(ErrorPopup::FatalDetailed(_, _)) => return,
            _ => {}
        }
        match error {
            ErrorPopup::Fatal(_) | ErrorPopup::FatalDetailed(_, _) => {
                self.error_message = Some(error);
                // Tell actors to stop, but let user close UI
                self.cancel_actors.cancel();
                // Just for the UI, "stop" the scan
                self.ble_scan_paused.store(true, Ordering::SeqCst);
            }
            // Don't let an intermittent error override a "UserMustDismiss" error
            ErrorPopup::Intermittent(_) => match self.error_message {
                Some(ErrorPopup::UserMustDismiss(_)) => {}
                _ => self.error_message = Some(error),
            },
            _ => self.error_message = Some(error),
        }
    }

    /// Terminal interval tick
    pub fn term_tick(&mut self) {
        (self.frame_count, _) = self.frame_count.overflowing_add(1);
    }

    pub fn scroll_up(&mut self) {
        match self.sub_state {
            SubState::CharacteristicView => {
                self.characteristic_scroll = self.characteristic_scroll.saturating_sub(1);
            }
            SubState::SaveDevicePrompt => {
                table_state_scroll(true, &mut self.prompt_state, 3);
            }
            #[cfg(windows)]
            SubState::VrcxAutostartPrompt => {
                table_state_scroll(true, &mut self.prompt_state, 4);
            }
            SubState::ActivitySelection => {
                table_state_scroll(
                    true,
                    &mut self.activities.table_state,
                    self.activities.query.len(),
                );
            }
            SubState::UpdateFoundPrompt | SubState::UpdateAllowCheckPrompt => {
                self.updates_scroll(true)
            }
            #[cfg(windows)]
            SubState::LaunchUpdatePrompt => self.updates_scroll(true),
            _ => {}
        }
        match self.view {
            AppView::BleDeviceSelection if self.is_idle_on_ble_selection() => {
                table_state_scroll(true, &mut self.table_state, self.discovered_devices.len());
            }
            _ => {}
        }
    }
    pub fn scroll_down(&mut self) {
        match self.sub_state {
            SubState::CharacteristicView => {
                self.characteristic_scroll = self.characteristic_scroll.wrapping_add(1);
            }
            SubState::SaveDevicePrompt => {
                table_state_scroll(false, &mut self.prompt_state, 3);
            }
            #[cfg(windows)]
            SubState::VrcxAutostartPrompt => {
                table_state_scroll(false, &mut self.prompt_state, 4);
            }
            SubState::ActivitySelection => {
                table_state_scroll(
                    false,
                    &mut self.activities.table_state,
                    self.activities.query.len(),
                );
            }
            SubState::UpdateFoundPrompt | SubState::UpdateAllowCheckPrompt => {
                self.updates_scroll(false)
            }
            #[cfg(windows)]
            SubState::LaunchUpdatePrompt => self.updates_scroll(false),
            _ => {}
        }
        match self.view {
            AppView::BleDeviceSelection if self.is_idle_on_ble_selection() => {
                table_state_scroll(false, &mut self.table_state, self.discovered_devices.len());
            }
            _ => {}
        }
    }
    pub fn escape_pressed(&mut self) {
        match self.sub_state {
            SubState::ActivitySelection | SubState::ActivityCreation => {
                self.activities_esc_pressed();
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
                ErrorPopup::Fatal(_) | ErrorPopup::FatalDetailed(_, _) => {
                    self.cancel_app.cancel();
                }
            }
            // Skip other checks if we dismissed an error.
            return;
        }

        match self.sub_state {
            SubState::CharacteristicView => {
                self.sub_state = SubState::None;
                return;
            }
            SubState::SaveDevicePrompt => {
                let chosen_option = self.prompt_state.selected().unwrap_or(0);
                match SavePromptChoice::from(chosen_option as u8) {
                    SavePromptChoice::Yes => {
                        self.should_save_ble_device = true;
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
                return;
            }
            #[cfg(windows)]
            SubState::VrcxAutostartPrompt => {
                use crate::vrcx::tui::VrcxPromptChoice;

                let chosen_option = self.prompt_state.selected().unwrap_or(0);
                match VrcxPromptChoice::from(chosen_option as u8) {
                    VrcxPromptChoice::Yes => {
                        if let Err(e) = self.vrcx.create_shortcut() {
                            self.handle_error_update(ErrorPopup::Intermittent(format!(
                                "Failed to create VRCX shortcut: {e}"
                            )));
                        } else {
                            // Commented out since the prompt is skipped if a shortcut exists,
                            // since if the user removes the shortcut *or* moves the exe + config somewhere else,
                            // it wouldn't prompt to make a new one!
                            // self.settings.misc.vrcx_shortcut_prompt = false;
                            // self.try_save_settings();

                            // TODO Maybe make this a lil more graceful
                            self.handle_error_update(ErrorPopup::UserMustDismiss("Autostart shortcut created! Make sure the App Launcher is enabled in VRCX's Advanced settings!".to_string()));
                            self.auto_update_prompt();
                        }
                    }
                    VrcxPromptChoice::No => {
                        self.auto_update_prompt();
                    }
                    VrcxPromptChoice::NeverAsk => {
                        self.settings.misc.vrcx_shortcut_prompt = false;
                        self.try_save_settings();
                        self.auto_update_prompt();
                    }
                    VrcxPromptChoice::OpenFolder => {
                        if let Err(e) = opener::open(self.vrcx.path().unwrap()) {
                            self.handle_error_update(ErrorPopup::UserMustDismiss(format!(
                                "Failed to open VRCX's startup folder! {e}"
                            )));
                        }
                    }
                }
                return;
            }
            SubState::ActivitySelection | SubState::ActivityCreation => {
                self.activities_enter_pressed();
            }
            SubState::UpdateAllowCheckPrompt | SubState::UpdateFoundPrompt => {
                self.updates_enter_pressed();
                return;
            }
            #[cfg(windows)]
            SubState::LaunchUpdatePrompt => {
                self.updates_enter_pressed();
                return;
            }
            _ => {}
        }
        #[allow(clippy::single_match)]
        match self.view {
            AppView::BleDeviceSelection => {
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
                    if device.services.contains(&HEART_RATE_SERVICE_UUID) {
                        self.discovered_devices.push(device.clone());
                    }
                    // This filter used to be in scan.rs, but doing it here
                    // lets us connect to saved devices without checking their services (i.e. quicker)
                }

                // If the device is saved, connect to it
                if self.is_device_saved(Some(&device)) && self.is_idle_on_ble_selection() {
                    self.quick_connect_ui = true;
                    // I'm going to assume that if we find a set saved device,
                    // they're always going to want to update the value in case Name/MAC changes,
                    // even if they're weird and have set `never_ask_to_save` to true
                    self.should_save_ble_device = true;
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
                    debug!("Connecting to saved device, AppView: {:?}", self.view);
                    // app_state changed by method
                    self.connect_for_hr(Some(&device));
                } else {
                    self.try_save_device(None);
                }
            }
            DeviceUpdate::Characteristics(characteristics) => {
                self.selected_characteristics = characteristics;
                self.sub_state = SubState::CharacteristicView
            }
            DeviceUpdate::Error(error) => {
                error!("BLE Thread Error: {:?}", error.clone());
                if self.view == AppView::HeartRateView && self.datasets_empty() {
                    // Ignoring the intermittent ones when we're in the inbetween state
                } else {
                    // Don't override a fatal error
                    if !matches!(self.error_message, Some(ErrorPopup::Fatal(_))) {
                        self.error_message = Some(error);
                    }
                }
                if self.view == AppView::HeartRateView
                    || self.sub_state == SubState::ConnectingForHeartRate
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
                if self.sub_state == SubState::ConnectingForCharacteristics {
                    self.sub_state = SubState::CharacteristicView;
                } else {
                    // If it wasn't for characteristics, it's probably for HR
                    self.view = AppView::HeartRateView;
                }

                if self.view == AppView::HeartRateView {
                    if id == self.get_selected_device().unwrap().id {
                        info!("Connected to device {:?}, stopping BLE scan", id);
                        self.ble_scan_paused.store(true, Ordering::SeqCst);
                    }
                    self.try_save_device(None);
                }
            }
            DeviceUpdate::DisconnectedEvent(disconnected_id) => {
                self.error_message = Some(ErrorPopup::Intermittent(
                    "Disconnected from device!".to_string(),
                ));
                if (self.view == AppView::HeartRateView || self.is_idle_on_ble_selection())
                    && disconnected_id == self.get_selected_device().unwrap().id
                {
                    info!(
                        "Disconnected from device {:?}, resuming BLE scan",
                        disconnected_id
                    );
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
