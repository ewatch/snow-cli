#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[tokio::main]
#[allow(
    clippy::disallowed_methods,
    reason = "the binary root owns process termination after library error mapping"
)]
async fn main() {
    if let Err(error) = snow_cli::run_cli().await {
        let exit_code = snow_cli::error::write_anyhow_error_and_exit_code(error);
        std::process::exit(exit_code);
    }
}
