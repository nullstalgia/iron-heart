use iron_heart::{args::TopLevelCmd, run_tui, AppResult};

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
