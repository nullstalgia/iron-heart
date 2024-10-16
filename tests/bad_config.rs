use iron_heart::args::TopLevelCmd;

use tokio_util::sync::CancellationToken;

use ntest::timeout;

#[test_log::test(tokio::test)]
#[test_log(default_log_filter = "debug")]
#[ignore = "can't be concurrent"]
#[timeout(3000)] // 3s timeout
#[should_panic(expected = "explicit panic")]
async fn misspelled_bool() {
    let parent_token = CancellationToken::new();

    let arg_config = TopLevelCmd {
        config_override: Some("tests/test_configs/misspelled_bool.toml".into()),
        config_required: true,
        no_save: true,
        subcommands: None,
        skip_prompts: true,
    };

    iron_heart::run_headless(arg_config, parent_token)
        .await
        .unwrap();
}

#[test_log::test(tokio::test)]
#[test_log(default_log_filter = "debug")]
#[ignore = "can't be concurrent"]
#[timeout(3000)] // 3s timeout
#[should_panic(expected = "explicit panic")]
async fn missing_end_quote() {
    let parent_token = CancellationToken::new();

    let arg_config = TopLevelCmd {
        config_override: Some("tests/test_configs/missing_end_quote.toml".into()),
        config_required: true,
        no_save: true,
        subcommands: None,
        skip_prompts: true,
    };

    iron_heart::run_headless(arg_config, parent_token)
        .await
        .unwrap();
}
