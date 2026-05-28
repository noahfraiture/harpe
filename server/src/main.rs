use harpe_server::config::AppConfig;
use harpe_server::runtime::{init_tracing, serve, shutdown_signal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    let config = AppConfig::from_env()?;

    serve(config, shutdown_signal()).await
}
