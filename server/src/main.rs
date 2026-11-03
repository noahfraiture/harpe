use harpe_server::config::AppConfig;
use harpe_server::runtime::{serve, shutdown_signal};
use harpe_server::telemetry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::from_env()?;
    let telemetry = telemetry::init()?;
    let result = serve(config, shutdown_signal()).await;

    if let Some(telemetry) = telemetry {
        telemetry.shutdown().await?;
    }
    result
}
