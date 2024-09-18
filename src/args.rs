use argh::FromArgs;
use std::path::PathBuf;

#[derive(FromArgs, Debug)]
/// Optional command line arguments
pub struct TopLevelCmd {
    /// specify config file path, creates file if it doesn't exist
    #[argh(option, short = 'c')]
    pub config_override: Option<PathBuf>,
    /// config file must exist, including "config_override" files
    #[argh(switch, short = 'r')]
    pub config_required: bool,
    /// use config file as-is (don't save over it)
    #[argh(switch, short = 'n')]
    pub no_save: bool,
    #[argh(subcommand)]
    pub subcommands: Option<SubCommands>,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand)]
pub enum SubCommands {
    Ble(BleCmd),
    WebSocket(WebSocketCmd),
    Dummy(DummyCmd),
}

/// connect to a BLE device with the HR Measure characteristic
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "ble")]
pub struct BleCmd {}

/// host a websocket server for HR sources to connect to
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "ws")]
pub struct WebSocketCmd {
    /// specify the port to listen on, otherwise uses config's port
    #[argh(option, short = 'p')]
    pub port: Option<u16>,
}

/// send dummy data for testing avatars/logging
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "dummy")]
pub struct DummyCmd {}
