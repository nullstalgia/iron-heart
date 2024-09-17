use iron_heart::{run_headless, ArgConfig};
use tokio_util::sync::CancellationToken;

// I have to spawn a tokio runtime for the app
// as #[tokio::test], even with "multi_thread" flavor
// will only spawn one of the child tasks.
// Not sure why!
#[allow(dead_code)]
pub fn headless_thread(
    arg_config: ArgConfig,
    parent_token: CancellationToken,
) -> Result<(), iron_heart::errors::AppError> {
    // TODO: Supply an mpsc for notifications?
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move {
            run_headless(arg_config, parent_token).await?;
            Ok(())
        })
}
