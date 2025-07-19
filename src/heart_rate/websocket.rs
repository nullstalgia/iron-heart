use super::twitcher::Twitcher;
use super::{BatteryLevel, HeartRateStatus};
use crate::app::{AppUpdate, ErrorPopup};
use crate::broadcast;
use crate::errors::AppError;
use crate::settings::WebSocketSettings;

use serde::Deserialize;
use std::net::{SocketAddr, SocketAddrV4};
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::broadcast::Sender as BSender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_websockets::{Message, ServerBuilder};

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

struct WebsocketActor {
    listener: TcpListener,
    hr_status: HeartRateStatus,
    twitcher: Twitcher,
    no_packet_timeout: Duration,
}

impl WebsocketActor {
    async fn build(
        websocket_settings: WebSocketSettings,
        port_override: Option<u16>,
        rr_twitch_threshold: f32,
        no_packet_timeout: Duration,
    ) -> Result<(Self, SocketAddr), AppError> {
        let port = port_override.unwrap_or(websocket_settings.port);
        let host_addr = SocketAddrV4::from_str(&format!("0.0.0.0:{port}"))?;

        let hr_status = HeartRateStatus {
            battery_level: BatteryLevel::NotReported,
            ..Default::default()
        };

        let listener = TcpListener::bind(host_addr).await?;

        let local_addr = listener.local_addr()?;

        Ok((
            Self {
                listener,
                hr_status,
                twitcher: Twitcher::new(rr_twitch_threshold),
                no_packet_timeout,
            },
            local_addr,
        ))
    }
    async fn server_loop(
        &mut self,
        broadcast_tx: &BSender<AppUpdate>,
        cancel_token: CancellationToken,
    ) -> Result<(), AppError> {
        'server: loop {
            info!("Websocket server (re)starting!");
            let connection: tokio::net::TcpStream;
            tokio::select! {
                result = self.listener.accept() => {
                    match result {
                        Ok((conn, _)) => {
                            connection = conn;
                        }
                        Err(err) => {
                            broadcast!(broadcast_tx, ErrorPopup::UserMustDismiss(format!(
                                "Handshake failed: {err:?}"

                            )));
                            continue 'server;
                        }
                    }
                }
                _ = cancel_token.cancelled() => {
                    info!("Shutting down Websocket thread!");
                    return Ok(());
                }
            }
            let mut stream = match ServerBuilder::new().accept(connection).await {
                Ok((_request, stream)) => stream,
                Err(err) => {
                    error!("Handshake failed: {err:?}",);
                    broadcast!(
                        broadcast_tx,
                        ErrorPopup::UserMustDismiss(format!("Handshake failed: {err:?}"))
                    );
                    continue 'server;
                }
            };
            debug!("Websocket handshake complete, starting rx loop.");
            'receiving: loop {
                tokio::select! {
                    item = stream.next() => {
                        if let Some((message, keep_conn)) = self.handle_ws_message(item) {
                            broadcast!(broadcast_tx, message);
                            if !keep_conn {
                                break 'receiving;
                            }
                        }
                    }
                    _ = tokio::time::sleep(self.no_packet_timeout) => {
                        let secs = self.no_packet_timeout.as_secs();
                        error!("No HR data received in {secs} seconds!");
                        broadcast!(
                            broadcast_tx,
                            ErrorPopup::Intermittent(format!("No HR data received in {secs} seconds!"))
                        );
                        break 'receiving;
                    }
                    _ = cancel_token.cancelled() => {
                        info!("Shutting down Websocket thread!");
                        stream.close().await?
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
    ) -> Option<(AppUpdate, bool)> {
        let message = match item {
            // Got a text-type message!
            Some(Ok(msg)) if msg.is_text() => {
                let msg = msg.as_text().unwrap().to_owned();
                msg
            }
            Some(Ok(msg)) if msg.is_close() => {
                warn!("Websocket client sent close opcode!");
                return Some((
                    ErrorPopup::Intermittent("Device closed connection!".to_string()).into(),
                    false,
                ));
            }
            Some(Ok(msg)) if msg.is_ping() || msg.is_pong() => {
                return None;
            }
            Some(Ok(msg)) => {
                error!("Invalid message type: {:?}", msg);
                return Some((
                    ErrorPopup::UserMustDismiss(format!(
                        "Invalid message type (expected text): {msg:?}"
                    ))
                    .into(),
                    true,
                ));
            }
            Some(Err(e)) => {
                error!("Error receiving message: {:?}", e);
                return Some((
                    ErrorPopup::Intermittent(format!("Error receiving message: {e:?}")).into(),
                    false,
                ));
            }
            None => {
                info!("Websocket client disconnected");
                return Some((
                    ErrorPopup::Intermittent("Websocket client disconnected".to_string()).into(),
                    false,
                ));
            }
        };

        if let Ok(new_status) = serde_json::from_str::<JSONHeartRate>(&message) {
            let now = chrono::Local::now();
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

            let (twitch_up, twitch_down) = self
                .twitcher
                .handle(new_status.bpm, &self.hr_status.rr_intervals);
            self.hr_status.twitch_up = twitch_up;
            self.hr_status.twitch_down = twitch_down;
            self.hr_status.timestamp = now;

            Some((self.hr_status.clone().into(), true))
        } else {
            error!("Invalid heart rate message: {message}");
            Some((
                AppUpdate::Error(ErrorPopup::Intermittent(format!(
                    "Invalid heart rate message: {message}"
                ))),
                true,
            ))
        }
    }
}

pub async fn websocket_thread(
    broadcast_tx: BSender<AppUpdate>,
    websocket_settings: WebSocketSettings,
    port_override: Option<u16>,
    rr_twitch_threshold: f32,
    no_packet_timeout: Duration,
    cancel_token: CancellationToken,
) {
    let (mut websocket, local_addr) = match WebsocketActor::build(
        websocket_settings,
        port_override,
        rr_twitch_threshold,
        no_packet_timeout,
    )
    .await
    {
        Ok((ws, addr)) => (ws, addr),
        Err(e) => {
            let message = "Failed to build websocket.";
            broadcast!(broadcast_tx, ErrorPopup::detailed(message, e));
            return;
        }
    };

    // Sharing the URL with the UI
    broadcast!(broadcast_tx, local_addr);

    if let Err(e) = websocket.server_loop(&broadcast_tx, cancel_token).await {
        error!("Websocket server error: {e}");
        let message = "Websocket server error";
        broadcast!(broadcast_tx, ErrorPopup::detailed(message, e));
    }
}
