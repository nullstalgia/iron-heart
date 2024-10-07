use std::path::PathBuf;

/// Represents all possible errors that can occur during the app's lifecycle
#[derive(Debug, thiserror::Error)]
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
    #[error("Failed to write to file \"{path}\": {source}")]
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
    },
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
    #[error("TOML Write Error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("TOML Read Error: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("CSV Error: {0}")]
    Csv(#[from] csv_async::Error),
    #[error("Parse Int Error: {0}")]
    ParseIntError(#[from] std::num::ParseIntError),
    #[error("Updater Error: {0}")]
    Updater(#[from] self_update::errors::Error),
    #[error("Task Join Error: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("Web Error: {0}")]
    Reqwest(#[from] reqwest::Error),
    // My errors
    #[error("Failed to get working directory")]
    WorkDir,
    #[error("Invalid OSC Prefix: \"{0}\"")]
    OscPrefix(String),
    #[error("Invalid OSC Address: \"{0}\" - \"{1}\"")]
    OscAddress(String, String),
    #[error("Failed to get event")]
    NoEvent,
    #[error("Bad HTTP Status: \"{0}\"")]
    HttpStatus(u16),
    #[error("Update checksum missing")]
    MissingChecksum,
    #[error("Update checksum mismatch")]
    BadChecksum,
    #[error("Tried to update non-portable app")]
    NotPortable,
    // Because lnk::Error doesn't impl Display yet
    #[error("Error parsing shortcut: {0}")]
    Lnk(String),
}
