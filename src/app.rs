use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use ratatui::widgets::TableState;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::{
    scan::{bluetooth_scan, get_characteristics},
    structs::{Characteristic, DeviceInfo},
};

pub enum DeviceData {
    DeviceInfo(DeviceInfo),
    #[allow(dead_code)]
    Characteristics(Vec<Characteristic>),
    Error(String),
}

pub struct App {
    pub rx: UnboundedReceiver<DeviceData>,
    pub tx: UnboundedSender<DeviceData>,
    pub loading_status: Arc<AtomicBool>,
    pub pause_status: Arc<AtomicBool>,
    pub table_state: TableState,
    pub devices: Vec<DeviceInfo>,
    pub inspect_view: bool,
    pub inspect_overlay_scroll: usize,
    pub selected_characteristics: Vec<Characteristic>,
    pub frame_count: usize,
    pub is_loading: bool,
    pub error_view: bool,
    pub error_message: String,
    pub heart_rate_bpm: u16,
    pub heart_rate_rr: Vec<u16>,
    pub monitor_battery: u8,
}

impl App {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            tx,
            rx,
            loading_status: Arc::new(AtomicBool::default()),
            pause_status: Arc::new(AtomicBool::default()),
            table_state: TableState::default(),
            devices: Vec::new(),
            inspect_view: false,
            inspect_overlay_scroll: 0,
            selected_characteristics: Vec::new(),
            frame_count: 0,
            is_loading: false,
            error_view: false,
            error_message: String::new(),
        }
    }

    pub async fn scan(&mut self) {
        let pause_signal_clone = Arc::clone(&self.pause_status);
        let tx_clone = self.tx.clone();
        tokio::spawn(async move { bluetooth_scan(tx_clone, pause_signal_clone).await });
    }

    pub async fn connect(&mut self) {
        let selected_device = self
            .devices
            .get(self.table_state.selected().unwrap_or(0))
            .unwrap();

        self.pause_status.store(true, Ordering::SeqCst);

        let device = Arc::new(selected_device.clone());
        let tx_clone = self.tx.clone();

        tokio::spawn(async move { get_characteristics(tx_clone, device).await });
    }
}
