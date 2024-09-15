use iron_heart::{run_tui, AppResult, ArgConfig};

#[tokio::main]
async fn main() -> AppResult<()> {
    let arg_config: ArgConfig = argh::from_env();
    if let Err(e) = run_tui(arg_config).await {
        eprintln!("An error occurred: {e}");
        Err(e)
    } else {
        Ok(())
    }
}
