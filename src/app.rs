use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use ratatui::widgets::TableState;
use tokio::sync::{
    mpsc::{self, UnboundedReceiver, UnboundedSender},
    Mutex,
};

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
    Error(String),
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

// #[derive(Debug, Clone)]
// pub enum ErrorMessage {
//     None,
//     Recoverable(String),
//     Fatal(String),
// }

// impl ErrorMessage {
//     pub fn is_none(&self) -> bool {
//         match self {
//             ErrorMessage::None => true,
//             _ => false,
//         }
//     }
// }

pub struct App {
    // Devices as found by the BLE thread
    pub ble_rx: UnboundedReceiver<DeviceData>,
    pub ble_tx: UnboundedSender<DeviceData>,
    // BLE Notifications from the heart rate monitor
    pub hr_rx: UnboundedReceiver<DeviceData>,
    pub hr_tx: UnboundedSender<DeviceData>,
    // Sending data to the OSC thread
    pub osc_rx: Arc<Mutex<UnboundedReceiver<DeviceData>>>,
    pub osc_tx: UnboundedSender<DeviceData>,
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
    pub error_message: Option<String>,
    pub settings: Settings,
    pub heart_rate_display: bool,
    pub heart_rate_status: HeartRateStatus,
}

impl App {
    pub fn new() -> Self {
        let (app_tx, app_rx) = mpsc::unbounded_channel();
        let (hr_tx, hr_rx) = mpsc::unbounded_channel();
        let (osc_tx, osc_rx) = mpsc::unbounded_channel();
        let settings = Settings::new().unwrap();
        Self {
            ble_tx: app_tx,
            ble_rx: app_rx,
            hr_tx: hr_tx,
            hr_rx: hr_rx,
            osc_tx: osc_tx,
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
            error_message: None,
            settings,
            heart_rate_display: false,
            heart_rate_status: HeartRateStatus::default(),
        }
    }

    pub async fn start_bluetooth_event_thread(&mut self) {
        let pause_signal_clone = Arc::clone(&self.ble_scan_paused);
        let app_tx_clone = self.ble_tx.clone();
        tokio::spawn(async move { bluetooth_event_thread(app_tx_clone, pause_signal_clone).await });
    }

    pub async fn connect_for_characteristics(&mut self) {
        let selected_device = self
            .get_selected_device()
            .expect("This crash is expected if discovered_devices is empty");

        self.ble_scan_paused.store(true, Ordering::SeqCst);

        let device = Arc::new(selected_device.clone());
        let app_tx_clone = self.ble_tx.clone();

        tokio::spawn(async move { get_characteristics(app_tx_clone, device).await });
    }

    pub async fn connect_for_hr(&mut self, quick_connect_device: Option<&DeviceInfo>) {
        let selected_device = if let Some(device) = quick_connect_device {
            self.state = AppState::ConnectingForHeartRate;
            device
        } else {
            // Let's check if we're okay asking to saving this device
            if !self.settings.ble.never_ask_to_save && self.state != AppState::SaveDevicePrompt {
                self.state = AppState::SaveDevicePrompt;
                return;
            }

            self.state = AppState::ConnectingForHeartRate;

            // Need to check if discovered devices is empty first
            // (not yet fixed as it's a good test crash for the panic handler)
            self.get_selected_device()
                .expect("This crash is expected if discovered_devices is empty")
        };

        self.ble_scan_paused.store(true, Ordering::SeqCst);

        let device = Arc::new(selected_device.clone());
        let hr_tx_clone = self.hr_tx.clone();

        // TODO handle here if shit panics
        tokio::spawn(async move { start_notification_thread(hr_tx_clone, device).await });
    }

    pub async fn start_osc_thread(&mut self) {
        let osc_rx_clone = Arc::clone(&self.osc_rx);
        let osc_settings = self.settings.osc.clone();
        tokio::spawn(async move { osc_thread(osc_rx_clone, osc_settings).await });
    }

    pub fn save_settings(&mut self) -> Result<(), std::io::Error> {
        self.settings.save()
    }

    pub fn get_selected_device(&self) -> Option<&DeviceInfo> {
        self.discovered_devices
            .get(self.table_state.selected().unwrap_or(0))
    }
}
