use argh::FromArgs;
use futures_util::SinkExt;
use http::Uri;
use serde::Serialize;
use tokio_websockets::{ClientBuilder, Message};

#[derive(FromArgs)]
/// iron-heart websocket tester
struct WsDummyArgs {
    /// specify the address to connect to (default: 127.0.0.1)
    #[argh(option, default = "String::from(\"127.0.0.1\")", short = 'a')]
    address: String,
    /// specify the port to connect to (default: 5566)
    #[argh(option, default = "5566", short = 'p')]
    port: u16,
    /// how many seconds between messages (default: 1s)
    #[argh(option, default = "1.0", short = 's')]
    speed: f32,
    /// don't send RR data
    #[argh(switch, short = 'n')]
    no_rr: bool,
}

#[derive(Serialize)]
#[allow(non_snake_case)]
struct JSONHeartRate {
    heartRate: u16,
    latest_rr_ms: Option<u64>,
    battery: u8,
}

impl From<u16> for JSONHeartRate {
    fn from(hr: u16) -> Self {
        Self {
            heartRate: hr,
            latest_rr_ms: Some(60000 / hr as u64),
            battery: 100,
        }
    }
}

#[tokio::main]
async fn main() {
    let args: WsDummyArgs = argh::from_env();
    let addr = format!("ws://{}:{}", args.address, args.port);
    println!("Connecting to {addr}...");

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
    let mut hr: JSONHeartRate = bpm_min.into();

    loop {
        hr.heartRate += 1;
        if hr.heartRate > bpm_max {
            hr.heartRate = bpm_min;
        }
        if args.no_rr {
            hr.latest_rr_ms = None;
        } else {
            hr.latest_rr_ms = Some(60000 / hr.heartRate as u64);
        }
        let json = serde_json::to_string(&hr).unwrap();
        client.send(Message::text(json)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs_f32(args.speed)).await;
    }
}
