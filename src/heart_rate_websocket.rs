use crate::app::{AppUpdate, ErrorPopup};
use crate::errors::AppError;
use crate::heart_rate::{BatteryLevel, HeartRateStatus};
use crate::settings::WebSocketSettings;

use log::*;
use serde::Deserialize;
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::str::FromStr;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::broadcast::Sender as BSender;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_websockets::{Message, ServerBuilder, WebSocketStream};

#[derive(Debug, Deserialize)]
struct JSONHeartRate {
    #[serde(alias = "heartrate", alias = "heartRate")]
    bpm: u16,
    // Options since no guarantee they'll exist
    latest_rr_ms: Option<u64>,
    battery: Option<u8>,
}

// TODO Add support for HeartRateOnStream, can use this as a reference: (thanks Curtis)
// (Need to mimic an OBS instance, agh)
// https://github.com/Curtis-VL/HeartRateOnStream-OSC/blob/main/Program.cs

// TODO Twitches

struct WebsocketActor {
    listener: TcpListener,
    hr_status: HeartRateStatus,
}

impl WebsocketActor {
    async fn build(websocket_settings: WebSocketSettings) -> Result<Self, AppError> {
        let port = websocket_settings.port;
        let host_addr = SocketAddrV4::from_str(&format!("0.0.0.0:{}", port))?;

        let hr_status = HeartRateStatus {
            battery_level: BatteryLevel::NotReported,
            ..Default::default()
        };

        let listener = TcpListener::bind(host_addr).await?;

        Ok(Self {
            listener,
            hr_status,
        })
    }
    async fn server_loop(
        &mut self,
        hr_tx: &BSender<AppUpdate>,
        cancel_token: CancellationToken,
    ) -> Result<(), AppError> {
        'server: loop {
            let connection: tokio::net::TcpStream;
            tokio::select! {
                result = self.listener.accept() => {
                    match result {
                        Ok((conn, _)) => {
                            connection = conn;
                        }
                        Err(err) => {
                            error!("Failed to accept connection: {}", err);
                            hr_tx
                                .send(AppUpdate::Error(ErrorPopup::UserMustDismiss(format!(
                                    "Handshake failed: {:?}",
                                    err
                                )))).expect("Failed to send message");
                            continue 'server;
                        }
                    }
                }
                _ = cancel_token.cancelled() => {
                    info!("Shutting down Websocket thread!");
                    return Ok(());
                }
            }
            let mut server = match ServerBuilder::new().accept(connection).await {
                Ok(server) => server,
                Err(e) => {
                    error!("Handshake failed: {:?}", e);
                    hr_tx
                        .send(AppUpdate::Error(ErrorPopup::UserMustDismiss(format!(
                            "Handshake failed: {:?}",
                            e
                        ))))
                        .expect("Failed to send message");
                    continue 'server;
                }
            };
            'receiving: loop {
                tokio::select! {
                    item = server.next() => {
                        let (message, keep_conn) = self.handle_ws_message(item)?;
                        hr_tx.send(message).expect("Failed to send message");
                        if keep_conn == false {
                            break 'receiving;
                        }
                    }
                    _ = cancel_token.cancelled() => {
                        info!("Shutting down Websocket thread!");
                        server.close().await?
                    }
                }
            }
        }
    }

    // async fn recieving_loop<S: AsyncRead + AsyncWrite + Unpin>(
    //     &self,
    //     server: WebSocketStream<S>,
    // ) -> Result<(), AppError> {
    //     unimplemented!();
    // }

    fn handle_ws_message(
        &mut self,
        item: Option<Result<Message, tokio_websockets::Error>>,
    ) -> Result<(AppUpdate, bool), AppError> {
        let message = match item {
            // Got a text-type message!
            Some(Ok(msg)) if msg.is_text() => {
                let msg = msg.as_text().unwrap().to_owned();
                msg
            }
            //
            Some(Ok(msg)) => {
                error!("Invalid message type: {:?}", msg);
                return Ok((
                    AppUpdate::Error(ErrorPopup::UserMustDismiss(format!(
                        "Invalid message type (expected text): {:?}",
                        msg
                    ))),
                    true,
                ));
            }
            Some(Err(e)) => {
                error!("Error receiving message: {:?}", e);
                return Ok((
                    AppUpdate::Error(ErrorPopup::Intermittent(format!(
                        "Error receiving message: {:?}",
                        e
                    ))),
                    false,
                ));
                //break 'receiving;
            }
            None => {
                info!("Websocket client disconnected");
                return Ok((
                    AppUpdate::Error(ErrorPopup::Intermittent(
                        "Websocket client disconnected".to_string(),
                    )),
                    false,
                ));
                //break 'receiving;
            }
        };
        if let Ok(new_status) = serde_json::from_str::<JSONHeartRate>(&message) {
            self.hr_status.heart_rate_bpm = new_status.bpm;
            if let Some(battery) = new_status.battery {
                self.hr_status.battery_level = BatteryLevel::Level(battery);
            }
            if let Some(rr) = new_status.latest_rr_ms {
                while !self.hr_status.rr_intervals.is_empty() {
                    self.hr_status.rr_intervals.pop();
                }
                self.hr_status.rr_intervals.push(Duration::from_millis(rr));
            }
            return Ok((AppUpdate::HeartRateStatus(self.hr_status.clone()), true));
        } else {
            error!("Invalid heart rate message: {}", message);
            return Ok((
                AppUpdate::Error(ErrorPopup::Intermittent(format!(
                    "Invalid heart rate message: {}",
                    message
                ))),
                true,
            ));
        }
    }
}

pub async fn websocket_thread(
    hr_tx: BSender<AppUpdate>,
    websocket_settings: WebSocketSettings,
    cancel_token: CancellationToken,
) {
    let mut websocket = match WebsocketActor::build(websocket_settings).await {
        Ok(ws) => ws,
        Err(e) => {
            let message = format!("Failed to build websocket. {e}");
            hr_tx
                .send(AppUpdate::Error(ErrorPopup::Fatal(message)))
                .expect("Failed to send error message");
            return;
        }
    };

    // Sharing the URL with the UI
    hr_tx
        .send(AppUpdate::WebsocketReady(
            websocket.listener.local_addr().unwrap(),
        ))
        .expect("Failed to send ready message");

    if let Err(e) = websocket.server_loop(&hr_tx, cancel_token).await {
        error!("Websocket server error: {e}");
        let message = format!("Websocket server error: {e}");
        hr_tx
            .send(AppUpdate::Error(ErrorPopup::Fatal(message)))
            .expect("Failed to send error message");
    }
}
