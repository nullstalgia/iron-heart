use iron_heart::{AppResult, args::TopLevelCmd, run_tui};

#[tokio::main]
async fn main() -> AppResult<()> {
    let arg_config: TopLevelCmd = argh::from_env();
    if let Err(e) = run_tui(arg_config).await {
        eprintln!("An error occurred: {e}");
        Err(e)
    } else {
        Ok(())
    }
}
