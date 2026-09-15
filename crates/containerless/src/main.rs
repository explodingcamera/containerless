use containerless::{CliError, run_cli};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("containerless=info,containerless_core=info")),
        )
        .with_target(false)
        .without_time()
        .init();

    match run_cli(std::env::args_os()).await {
        Ok(()) => {}
        Err(CliError::Arguments(error)) => error.exit(),
        Err(error) => {
            eprintln!("containerless: {error}");
            std::process::exit(1);
        }
    }
}
