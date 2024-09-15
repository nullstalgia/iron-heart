use std::path::PathBuf;

use thiserror::Error;

/// Represents all possible errors that can occur during the app's lifecycle
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Failed to create directory \"{path}\": {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to create file \"{path}\": {source}")]
    CreateFile {
        path: PathBuf,
        source: std::io::Error,
    },
    // #[error("Failed to write to file \"{path}\": {source}")]
    // WriteFile {
    //     path: PathBuf,
    //     source: std::io::Error,
    // },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Error parsing IP Address: {0}")]
    AddrParse(#[from] std::net::AddrParseError),
    #[error("Error parsing config: {0}")]
    Config(#[from] config::ConfigError),
    #[error("OSC Error: {0}")]
    Osc(#[from] rosc::OscError),
    #[error("Websocket Error: {0}")]
    Ws(#[from] tokio_websockets::Error),
    #[error("Bluetooth Error: {0}")]
    Bt(#[from] btleplug::Error),
    #[error("TOML Serialization Error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("CSV Error: {0}")]
    Csv(#[from] csv_async::Error),
    // My errors
    #[error("Failed to get working directory")]
    WorkDir,
    #[error("Invalid OSC Prefix: \"{0}\"")]
    OscPrefix(String),
    #[error("Invalid OSC Address: \"{0}\" - \"{1}\"")]
    OscAddress(String, String),
}
