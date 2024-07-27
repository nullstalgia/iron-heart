use std::{
    f32::consts::PI,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use log::*;
use ratatui::widgets::TableState;
use tokio::sync::{
    mpsc::{self, UnboundedReceiver, UnboundedSender},
    Mutex,
};
use tokio_util::sync::CancellationToken;

use crate::{
    heart_rate::{start_notification_thread, HeartRateStatus},
    osc::osc_thread,
    scan::{bluetooth_event_thread, get_characteristics},
    settings::{OSCSettings, Settings},
    structs::{Characteristic, DeviceInfo},
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
    pub heart_rate_display: bool,
    pub heart_rate_status: HeartRateStatus,
    pub shutdown_requested: CancellationToken,
    pub ble_thread_handle: Option<tokio::task::JoinHandle<()>>,
    pub hr_thread_handle: Option<tokio::task::JoinHandle<()>>,
    pub osc_thread_handle: Option<tokio::task::JoinHandle<()>>,
    // Used for the graphs in the heart rate view
    pub heart_rate_history: Vec<u16>,
    pub rr_history: Vec<u16>,
}

impl App {
    pub fn new() -> Self {
        let (app_tx, app_rx) = mpsc::unbounded_channel();
        let (hr_tx, hr_rx) = mpsc::unbounded_channel();
        let (osc_tx, osc_rx) = mpsc::unbounded_channel();
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
            heart_rate_display: false,
            heart_rate_status: HeartRateStatus::default(),
            heart_rate_history: Vec::with_capacity(50),
            rr_history: Vec::with_capacity(50),
            shutdown_requested: CancellationToken::new(),
            ble_thread_handle: None,
            hr_thread_handle: None,
            osc_thread_handle: None,
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
        let selected_device = self
            .get_selected_device()
            .expect("This crash is expected if discovered_devices is empty");

        debug!("(C) Pausing BLE scan");
        self.ble_scan_paused.store(true, Ordering::SeqCst);

        let device = Arc::new(selected_device.clone());
        let app_tx_clone = self.ble_tx.clone();

        debug!("Spawning characteristics thread");
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
        let rr_twitch_threshold =
            Duration::from_millis(self.settings.osc.twitch_rr_threshold_ms as u64).as_secs_f32();
        debug!("Spawning notification thread, AppState: {:?}", self.state);
        self.hr_thread_handle = Some(tokio::spawn(async move {
            start_notification_thread(
                hr_tx_clone,
                device,
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

    pub async fn join_threads(&mut self) {
        use tokio::time::{timeout, Duration};
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
            if self.settings.ble.saved_address != new_id {
                self.settings.ble.saved_address = new_id;
                damaged = true;
            }
            // TODO See if I can find a way to get "Unknown" programatically,
            // not a fan of hardcoding it (and it's "" in the ::default())
            // Maybe do a .new() and supply a None?
            if self.settings.ble.saved_name != new_name && new_name != "Unknown" {
                self.settings.ble.saved_name = new_name;
                damaged = true;
            }
            if damaged {
                self.save_settings().expect("Failed to save settings");
            }
        }
    }

    pub fn get_selected_device(&self) -> Option<&DeviceInfo> {
        self.discovered_devices
            .get(self.table_state.selected().unwrap_or(0))
    }

    pub fn is_idle_on_main_menu(&self) -> bool {
        self.error_message.is_none() && self.state == AppState::MainMenu
    }
}
