use futures_util::SinkExt;
use http::Uri;
use serde::Serialize;
use tokio_websockets::{ClientBuilder, Message};

#[derive(Serialize)]
#[allow(non_snake_case)]
struct JSONHeartRate {
    heartRate: u16,
    latest_rr_ms: u64,
    battery: u8,
}

#[tokio::main]
async fn main() {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:5566".to_string());
    let addr = format!("ws://{}", addr);
    print!("Connecting to ");
    println!("{}...", addr);

    let uri = Uri::from_maybe_shared(addr).expect("Invalid URI supplied!");
    let (mut client, _) = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        ClientBuilder::from_uri(uri).connect(),
    )
    .await
    .expect("Connecting to websocket server timed out!")
    .expect("Failed to connect to websocket server!");

    println!("Connected to websocket server!");
    let bpm_min = 70;
    let bpm_max = 120;
    let mut hr = JSONHeartRate {
        heartRate: bpm_min,
        latest_rr_ms: 0,
        battery: 100,
    };
    //let rr = 0.0;

    loop {
        hr.heartRate += 1;
        if hr.heartRate > bpm_max {
            hr.heartRate = bpm_min;
        }
        hr.latest_rr_ms = 60000 / hr.heartRate as u64;
        let json = serde_json::to_string(&hr).unwrap();
        client.send(Message::text(json)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
