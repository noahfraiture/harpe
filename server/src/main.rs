use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use harpe_server::api::grpc::HarpeGrpc;
use harpe_server::db::surreal::SurrealStore;
use harpe_server::jobs::JobRunner;
use harpe_server::llm::EchoLlm;
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

    let addr = std::env::var("HARPE_GRPC_ADDR")
        .unwrap_or_else(|_| "[::1]:50051".to_owned())
        .parse::<SocketAddr>()?;
    let surreal_endpoint =
        std::env::var("SURREALDB_ENDPOINT").unwrap_or_else(|_| "memory".to_owned());
    let surreal_namespace =
        std::env::var("SURREALDB_NAMESPACE").unwrap_or_else(|_| "harpe".to_owned());
    let surreal_database = std::env::var("SURREALDB_DATABASE").unwrap_or_else(|_| "dev".to_owned());

    let store = Arc::new(
        SurrealStore::connect(surreal_endpoint, &surreal_namespace, &surreal_database).await?,
    );
    let llm = Arc::new(EchoLlm::development_default());
    let service = HarpeGrpc::new(store.clone(), llm.clone());
    let _job_worker = JobRunner::new(store, llm).spawn(Duration::from_secs(2), 25);

    info!(%addr, "starting harpe gRPC server");

    Server::builder()
        .add_service(UserServiceServer::new(service.clone()))
        .add_service(GameServiceServer::new(service.clone()))
        .add_service(SessionServiceServer::new(service.clone()))
        .add_service(MemoryServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
