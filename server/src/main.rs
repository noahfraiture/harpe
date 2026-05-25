use std::sync::Arc;

use harpe_server::api::grpc::HarpeGrpc;
use harpe_server::config::{AppConfig, AppLlmConfig};
use harpe_server::db::surreal::SurrealStore;
use harpe_server::jobs::JobRunner;
use harpe_server::llm::{EchoLlm, HttpLlm, LlmClient};
use harpe_server::pb::game_service_server::GameServiceServer;
use harpe_server::pb::memory_service_server::MemoryServiceServer;
use harpe_server::pb::session_service_server::SessionServiceServer;
use harpe_server::pb::user_service_server::UserServiceServer;
use tonic::transport::Server;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "harpe_server=info,tower_http=info".into()),
        )
        .init();

    let config = AppConfig::from_env()?;

    let store = Arc::new(
        SurrealStore::connect(
            config.surreal_endpoint,
            &config.surreal_namespace,
            &config.surreal_database,
        )
        .await?,
    );
    let llm: Arc<dyn LlmClient> = match config.llm {
        AppLlmConfig::Echo => Arc::new(EchoLlm::development_default()),
        AppLlmConfig::Http(http_config) => Arc::new(HttpLlm::new(http_config)?),
    };
    let service = HarpeGrpc::new(store.clone(), llm.clone());
    let _job_worker = JobRunner::new(store, llm).spawn(config.job_interval, config.job_batch_limit);

    let addr = config.grpc_addr;
    info!(%addr, "starting harpe gRPC server");

    Server::builder()
        .add_service(UserServiceServer::new(service.clone()))
        .add_service(GameServiceServer::new(service.clone()))
        .add_service(SessionServiceServer::new(service.clone()))
        .add_service(MemoryServiceServer::new(service))
        .serve_with_shutdown(addr, shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::warn!(%error, "failed to listen for shutdown signal");
    }

    info!("shutdown signal received");
}
