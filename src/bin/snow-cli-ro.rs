#[tokio::main]
async fn main() {
    if let Err(error) = snow_cli::run_read_only_cli().await {
        let exit_code = snow_cli::error::write_anyhow_error_and_exit_code(error);
        std::process::exit(exit_code);
    }
}
