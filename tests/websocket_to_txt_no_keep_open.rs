use std::{thread::sleep, time::Duration};

use iron_heart::args::TopLevelCmd;
use tokio_util::sync::CancellationToken;

use futures_util::SinkExt;
use http::Uri;
use serde::Serialize;
use tokio_websockets::{ClientBuilder, Message};

use ntest::timeout;

use common::headless_thread;
mod common;

#[derive(Serialize)]
#[allow(non_snake_case)]
struct JSONHeartRate {
    heartRate: u16,
    latest_rr_ms: u64,
    battery: u8,
}

impl From<u16> for JSONHeartRate {
    fn from(hr: u16) -> Self {
        Self {
            heartRate: hr,
            latest_rr_ms: 60000 / u64::from(hr),
            battery: 100,
        }
    }
}

#[test_log::test(tokio::test)]
#[test_log(default_log_filter = "debug")]
#[ignore = "can't be concurrent"]
#[timeout(10000)] // 10s timeout
async fn websocket_to_txt_no_keep_open() -> Result<(), iron_heart::errors::AppError> {
    let parent_token = CancellationToken::new();

    let arg_config = TopLevelCmd {
        config_override: Some("tests/test_configs/websocket_to_txt_no_keep_open.toml".into()),
        config_required: true,
        no_save: true,
        subcommands: None,
        skip_prompts: true,
    };

    let parent_clone = parent_token.clone();
    let app_thread = std::thread::spawn(move || headless_thread(arg_config, parent_clone));
    let addr = "ws://127.0.0.1:5566";
    let uri = Uri::from_maybe_shared(addr).expect("Invalid URI supplied!");
    sleep(Duration::from_millis(250));
    println!("App running");
    let (mut client, _) = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ClientBuilder::from_uri(uri).connect(),
    )
    .await
    .expect("Connecting to websocket server timed out!")
    .expect("Failed to connect to websocket server!");
    println!("Websocket connected");
    let json_hr = |hr: u16| {
        let hr_status: JSONHeartRate = hr.into();
        serde_json::to_string(&hr_status).unwrap()
    };

    let mut hr: u16 = 75;

    sleep(Duration::from_secs(1));
    println!("Sending messages");
    client.send(Message::text(json_hr(hr))).await.unwrap();
    sleep(Duration::from_millis(100));
    println!("Opening file");

    let file_dir = "tests/output/bpm_no_keep_open.txt";

    {
        let file_contents = tokio::fs::read_to_string(file_dir).await?;

        assert_eq!(file_contents.trim().parse::<u16>()?, hr);

        hr = 80;
        client.send(Message::text(json_hr(hr))).await.unwrap();
        sleep(Duration::from_millis(300));

        let file_contents = tokio::fs::read_to_string(file_dir).await?;

        assert_eq!(file_contents.trim().parse::<u16>()?, hr);

        hr = 500;
        client.send(Message::text(json_hr(hr))).await.unwrap();
        sleep(Duration::from_millis(300));

        let file_contents = tokio::fs::read_to_string(file_dir).await?;

        assert_eq!(file_contents.trim().parse::<u16>()?, hr);
    }

    println!("Shutting down, all ok");

    parent_token.cancel();
    std::fs::remove_file(file_dir)?;
    client.close().await?;
    let _ = app_thread.join();
    Ok(())
}
