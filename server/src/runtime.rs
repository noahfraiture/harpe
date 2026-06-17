use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::task::JoinHandle;
use tonic::transport::Server;
use tracing::info;

use crate::Result;
use crate::api::grpc::HarpeGrpc;
use crate::config::{AppConfig, AppLlmConfig};
use crate::db::surreal::{SurrealCredentials, SurrealStore};
use crate::jobs::JobRunner;
use crate::llm::{EchoLlm, HttpLlm, LlmClient};
use crate::observability::{AppMetrics, SharedMetrics};
use crate::pb::admin_service_server::AdminServiceServer;
use crate::pb::game_service_server::GameServiceServer;
use crate::pb::health_service_server::HealthServiceServer;
use crate::pb::memory_service_server::MemoryServiceServer;
use crate::pb::metrics_service_server::MetricsServiceServer;
use crate::pb::session_service_server::SessionServiceServer;
use crate::pb::user_service_server::UserServiceServer;
use crate::store::HarpeStore;

pub struct RuntimeParts {
    pub store: Arc<SurrealStore>,
    pub llm: Arc<dyn LlmClient>,
    pub metrics: SharedMetrics,
    pub service: HarpeGrpc,
}

pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "harpe_server=info,tower_http=info".into()),
        )
        .try_init();
}

pub async fn build_runtime_parts(config: &AppConfig) -> Result<RuntimeParts> {
    let store = store_from_config(config).await?;
    let llm = llm_from_config(config.llm.clone())?;
    let metrics = AppMetrics::shared();
    let service_store: Arc<dyn HarpeStore> = store.clone();
    let service = grpc_service(service_store, llm.clone(), metrics.clone());

    Ok(RuntimeParts {
        store,
        llm,
        metrics,
        service,
    })
}

pub async fn store_from_config(config: &AppConfig) -> Result<Arc<SurrealStore>> {
    let credentials = config
        .surreal_username
        .as_ref()
        .zip(config.surreal_password.as_ref())
        .map(|(username, password)| SurrealCredentials {
            username: username.clone(),
            password: password.clone(),
        });

    Ok(Arc::new(
        SurrealStore::connect_with_credentials(
            config.surreal_endpoint.clone(),
            &config.surreal_namespace,
            &config.surreal_database,
            credentials,
        )
        .await?,
    ))
}

pub fn llm_from_config(config: AppLlmConfig) -> Result<Arc<dyn LlmClient>> {
    match config {
        AppLlmConfig::Echo => Ok(Arc::new(EchoLlm::development_default())),
        AppLlmConfig::Http(http_config) => Ok(Arc::new(HttpLlm::new(http_config)?)),
    }
}

pub fn grpc_service(
    store: Arc<dyn HarpeStore>,
    llm: Arc<dyn LlmClient>,
    metrics: SharedMetrics,
) -> HarpeGrpc {
    HarpeGrpc::new(store, llm).with_metrics(metrics)
}

pub fn spawn_job_worker(
    store: Arc<dyn HarpeStore>,
    llm: Arc<dyn LlmClient>,
    metrics: SharedMetrics,
    interval: std::time::Duration,
    batch_limit: usize,
) -> JoinHandle<()> {
    JobRunner::new(store, llm)
        .with_metrics(metrics)
        .spawn(interval, batch_limit)
}

pub async fn serve(
    config: AppConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let parts = build_runtime_parts(&config).await?;
    let job_store: Arc<dyn HarpeStore> = parts.store.clone();
    let job_worker = spawn_job_worker(
        job_store,
        parts.llm,
        parts.metrics,
        config.job_interval,
        config.job_batch_limit,
    );

    let addr = config.grpc_addr;
    info!(%addr, "starting harpe gRPC server");
    let result = serve_grpc(addr, parts.service, shutdown).await;
    job_worker.abort();
    result?;

    Ok(())
}

pub async fn serve_grpc(
    addr: SocketAddr,
    service: HarpeGrpc,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::result::Result<(), tonic::transport::Error> {
    Server::builder()
        .add_service(AdminServiceServer::new(service.clone()))
        .add_service(HealthServiceServer::new(service.clone()))
        .add_service(MetricsServiceServer::new(service.clone()))
        .add_service(UserServiceServer::new(service.clone()))
        .add_service(GameServiceServer::new(service.clone()))
        .add_service(SessionServiceServer::new(service.clone()))
        .add_service(MemoryServiceServer::new(service))
        .serve_with_shutdown(addr, shutdown)
        .await
}

pub async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::warn!(%error, "failed to listen for shutdown signal");
    }

    info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uuid::Uuid;

    use super::*;
    use crate::HarpeError;
    use crate::llm::HttpLlmConfig;

    fn memory_config() -> AppConfig {
        AppConfig {
            grpc_addr: "127.0.0.1:0".parse().unwrap(),
            surreal_endpoint: "memory".to_owned(),
            surreal_namespace: "harpe".to_owned(),
            surreal_database: format!("runtime_test_{}", Uuid::now_v7()),
            surreal_username: None,
            surreal_password: None,
            llm: AppLlmConfig::Echo,
            job_interval: Duration::from_millis(50),
            job_batch_limit: 2,
        }
    }

    #[test]
    fn llm_from_config_builds_echo_and_validates_http_config() {
        assert!(llm_from_config(AppLlmConfig::Echo).is_ok());

        let invalid_http = AppLlmConfig::Http(HttpLlmConfig::openai_compatible(
            " ".to_owned(),
            None,
            "chat".to_owned(),
            "extract".to_owned(),
            "embed".to_owned(),
        ));
        assert!(matches!(
            llm_from_config(invalid_http),
            Err(HarpeError::Validation(_))
        ));
    }

    #[test]
    fn init_tracing_is_idempotent() {
        init_tracing();
        init_tracing();
    }

    #[tokio::test]
    async fn build_runtime_parts_connects_store_and_builds_service_metrics() {
        let config = memory_config();
        let parts = build_runtime_parts(&config).await.unwrap();

        assert_eq!(
            parts.store.applied_migrations().await.unwrap().len(),
            SurrealStore::migration_versions().len()
        );
        assert_eq!(parts.metrics.snapshot().grpc_requests, 0);
    }

    #[tokio::test]
    async fn serve_grpc_returns_when_shutdown_is_ready() {
        let config = memory_config();
        let parts = build_runtime_parts(&config).await.unwrap();

        serve_grpc(config.grpc_addr, parts.service, async {})
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn serve_returns_when_shutdown_is_ready() {
        let config = memory_config();

        serve(config, async {}).await.unwrap();
    }
}
