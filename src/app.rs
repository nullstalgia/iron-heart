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
    heart_rate::{subscribe_to_heart_rate, HeartRateStatus, MonitorData},
    osc::osc_thread,
    scan::{bluetooth_scan, get_characteristics},
    settings::{OSCSettings, Settings},
    structs::{Characteristic, DeviceInfo},
};

pub enum DeviceData {
    DeviceInfo(DeviceInfo),
    #[allow(dead_code)]
    Characteristics(Vec<Characteristic>),
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

pub struct App {
    pub app_rx: UnboundedReceiver<DeviceData>,
    pub app_tx: UnboundedSender<DeviceData>,
    pub hr_rx: UnboundedReceiver<MonitorData>,
    pub hr_tx: UnboundedSender<MonitorData>,
    pub osc_rx: Arc<Mutex<UnboundedReceiver<MonitorData>>>,
    pub osc_tx: UnboundedSender<MonitorData>,
    pub ble_scan_paused: Arc<AtomicBool>,
    pub app_state: AppState,
    pub table_state: TableState,
    pub discovered_devices: Vec<DeviceInfo>,
    pub selected_device: Option<usize>,
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
            app_tx: app_tx,
            app_rx: app_rx,
            hr_tx: hr_tx,
            hr_rx: hr_rx,
            osc_tx: osc_tx,
            osc_rx: Arc::new(Mutex::new(osc_rx)),
            ble_scan_paused: Arc::new(AtomicBool::default()),
            app_state: AppState::MainMenu,
            table_state: TableState::default(),
            discovered_devices: Vec::new(),
            selected_device_index: None,
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

    pub async fn scan(&mut self) {
        let pause_signal_clone = Arc::clone(&self.ble_scan_paused);
        let app_tx_clone = self.app_tx.clone();
        tokio::spawn(async move { bluetooth_scan(app_tx_clone, pause_signal_clone).await });
    }

    pub async fn connect_for_characteristics(&mut self) {
        self.selected_device_index = self.table_state.selected();
        let selected_device = self
            .discovered_devices
            .get(self.selected_device_index.unwrap_or(0))
            .unwrap();

        self.ble_scan_paused.store(true, Ordering::SeqCst);

        let device = Arc::new(selected_device.clone());
        let app_tx_clone = self.app_tx.clone();

        tokio::spawn(async move { get_characteristics(app_tx_clone, device).await });
    }

    pub async fn connect_for_hr(&mut self, quick_connect_device: Option<DeviceInfo>) {
        let selected_device = if let Some(device) = quick_connect_device {
            self.app_state = AppState::ConnectingForHeartRate;
            device
        } else {
            // Need to check if discovered devices is empty first
            // (not yet fixed as it's a good test crash for the panic handler)
            self.selected_device_index = self.table_state.selected();
            self.discovered_devices
                .get(self.selected_device_index.unwrap_or(0))
                .unwrap()
                .clone()
        };

        self.ble_scan_paused.store(true, Ordering::SeqCst);

        let device = Arc::new(selected_device.clone());
        let hr_tx_clone = self.hr_tx.clone();

        tokio::spawn(async move { subscribe_to_heart_rate(hr_tx_clone, device).await });
    }

    pub async fn start_osc_thread(&mut self, osc_settings: OSCSettings) {
        let osc_rx_clone = Arc::clone(&self.osc_rx);
        tokio::spawn(async move { osc_thread(osc_rx_clone, osc_settings).await });
    }

    pub fn save_settings(&mut self) -> Result<(), std::io::Error> {
        self.settings.save()
    }
}
