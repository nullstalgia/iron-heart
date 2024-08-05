use crate::app::{DeviceData, ErrorPopup};
use crate::heart_rate::{BatteryLevel, HeartRateStatus};
use crate::settings::WebSocketSettings;

use log::*;
use serde::Deserialize;
use std::net::SocketAddrV4;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_websockets::ServerBuilder;

#[derive(Debug, Deserialize)]
struct JSONHeartRate {
    #[serde(alias = "heartrate", alias = "heartRate")]
    bpm: u16,
    latest_rr_ms: Option<u64>,
    battery: Option<u8>,
}

// TODO Add support for HeartRateOnStream, can use this as a reference: (thanks Curtis)
// (Need to mimic an OBS instance, agh)
// https://github.com/Curtis-VL/HeartRateOnStream-OSC/blob/main/Program.cs

pub async fn websocket_thread(
    hr_tx: mpsc::UnboundedSender<DeviceData>,
    websocket_settings: WebSocketSettings,
    shutdown_token: CancellationToken,
) {
    let port = websocket_settings.port;
    let host_addr = SocketAddrV4::from_str(&format!("0.0.0.0:{}", port))
        .expect("Invalid websocket host IP address!");

    let listener = match TcpListener::bind(host_addr).await {
        Ok(listener) => listener,
        Err(e) => {
            error!("Could not bind to TCP port {}! {}", port, e);
            hr_tx
                .send(DeviceData::Error(ErrorPopup::Fatal(format!(
                    "Could not bind to TCP port {}!",
                    port
                ))))
                .expect("Failed to send error message");
            return;
        }
    };

    hr_tx
        .send(DeviceData::WebsocketReady(listener.local_addr().unwrap()))
        .expect("Failed to send ready message");

    let mut hr_status = HeartRateStatus {
        battery_level: BatteryLevel::NotReported,
        ..Default::default()
    };

    'server: loop {
        let connection: tokio::net::TcpStream;
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((conn, _)) => {
                        connection = conn;
                    }
                    Err(e) => {
                        error!("Failed to accept connection: {}", e);
                        hr_tx
                            .send(DeviceData::Error(ErrorPopup::UserMustDismiss(format!(
                                "Handshake failed: {:?}",
                                e
                            ))))
                            .expect("Failed to send error message");
                        continue 'server;
                    }
                }
            }
            _ = shutdown_token.cancelled() => {
                info!("Shutting down Websocket thread!");
                break 'server;
            }
        }
        let mut server = match ServerBuilder::new().accept(connection).await {
            Ok(server) => server,
            Err(e) => {
                error!("Handshake failed: {:?}", e);
                hr_tx
                    .send(DeviceData::Error(ErrorPopup::UserMustDismiss(format!(
                        "Handshake failed: {:?}",
                        e
                    ))))
                    .expect("Failed to send error message");
                continue 'server;
            }
        };

        'receiving: loop {
            tokio::select! {
                item = server.next() => {
                    match item {
                        Some(Ok(message)) => {
                            if message.is_text() {
                                let text = message.as_text().unwrap();
                                if let Ok(hr) = serde_json::from_str::<JSONHeartRate>(text) {
                                    hr_status.heart_rate_bpm = hr.bpm;
                                    if let Some(battery) = hr.battery {
                                        hr_status.battery_level = BatteryLevel::Level(battery);
                                    }
                                    if let Some(rr) = hr.latest_rr_ms {
                                        while !hr_status.rr_intervals.is_empty() {
                                            hr_status.rr_intervals.pop();
                                        }
                                        hr_status.rr_intervals.push(Duration::from_millis(rr));
                                    }
                                    hr_tx.send(DeviceData::HeartRateStatus(hr_status.clone())).expect("Failed to send heart rate message");
                                } else {
                                    error!("Invalid heart rate message: {}", text);
                                    hr_tx
                                        .send(DeviceData::Error(ErrorPopup::Intermittent(format!(
                                            "Invalid heart rate message: {}", text
                                        ))))
                                        .expect("Failed to send error message");
                                }
                            } else {
                                error!("Invalid message type: {:?}", message);
                                hr_tx
                                    .send(DeviceData::Error(ErrorPopup::UserMustDismiss(
                                        format!("Invalid message type (expected text): {:?}", message),
                                    )))
                                    .expect("Failed to send error message");
                            }
                        }
                        Some(Err(e)) => {
                            error!("Error receiving message: {:?}", e);
                            hr_tx
                                .send(DeviceData::Error(ErrorPopup::Intermittent(format!(
                                    "Error receiving message: {:?}", e
                                ))))
                                .expect("Failed to send error message");
                            break 'receiving;
                        }
                        None => {
                            info!("Client disconnected");
                            hr_tx
                                .send(DeviceData::Error(ErrorPopup::Intermittent("Client disconnected".to_string())))
                                .expect("Failed to send error message");
                            break 'receiving;
                        }
                    }
                }
                _ = shutdown_token.cancelled() => {
                    info!("Shutting down Websocket thread!");
                    server.close().await.expect("Failed to close websocket connection");
                    break 'server;
                }
            }
        }
    }
}
