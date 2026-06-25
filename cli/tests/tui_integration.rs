use std::process::Command;
use std::sync::Arc;

use harpe_server::api::grpc::HarpeGrpc;
use harpe_server::db::surreal::SurrealStore;
use harpe_server::domain::{NewGame, NewSession, NewUser};
use harpe_server::llm::EchoLlm;
use harpe_server::pb::admin_service_server::AdminServiceServer;
use harpe_server::pb::game_service_server::GameServiceServer;
use harpe_server::pb::health_service_server::HealthServiceServer;
use harpe_server::pb::memory_service_server::MemoryServiceServer;
use harpe_server::pb::metrics_service_server::MetricsServiceServer;
use harpe_server::pb::session_service_server::SessionServiceServer;
use harpe_server::pb::user_service_server::UserServiceServer;
use harpe_server::store::HarpeStore;
use rexpect::session::spawn_command;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use uuid::Uuid;

#[test]
fn tui_help_exposes_runtime_options_and_main_key_concepts() {
    let output = Command::new(env!("CARGO_BIN_EXE_harpe-tui"))
        .arg("--help")
        .output()
        .expect("run harpe-tui --help");

    assert!(
        output.status.success(),
        "harpe-tui --help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("help output is utf8");
    assert!(stdout.contains("Terminal roleplay cockpit"));
    assert!(stdout.contains("--addr"));
    assert!(stdout.contains("--user-id"));
    assert!(stdout.contains("--game-id"));
    assert!(stdout.contains("--session-id"));
    assert!(stdout.contains("--model"));
}

#[tokio::test(flavor = "multi_thread")]
async fn tui_runs_against_real_grpc_server_and_sends_a_turn() {
    let store = Arc::new(test_store().await);
    let llm = Arc::new(EchoLlm::new(vec![
        "The gate ".to_owned(),
        "opens.".to_owned(),
    ]));
    let service = HarpeGrpc::new(store.clone(), llm);
    let (addr, server) = spawn_grpc_service(service).await;

    let user = store
        .create_user(NewUser {
            display_name: "Noah".to_owned(),
        })
        .await
        .unwrap();
    let game = store
        .create_game(NewGame {
            owner_user_id: user.id.clone(),
            title: "Iron Coast".to_owned(),
            system_prompt: "Run a coastal fantasy adventure.".to_owned(),
        })
        .await
        .unwrap();
    let session = store
        .create_session(NewSession {
            game_id: game.id.clone(),
            title: "First watch".to_owned(),
        })
        .await
        .unwrap();
    let config_path = std::env::temp_dir().join(format!("harpe-tui-e2e-{}.toml", Uuid::now_v7()));

    let bin = env!("CARGO_BIN_EXE_harpe-tui").to_owned();
    let addr_for_tui = addr.clone();
    let user_id = user.id.clone();
    let session_id = session.id.clone();
    let config_path_for_tui = config_path.clone();
    let test_result = tokio::task::spawn_blocking(move || {
        let mut command = Command::new("sh");
        command
            .env("TERM", "xterm-256color")
            .env("COLUMNS", "130")
            .env("LINES", "40")
            .arg("-lc")
            .arg("stty rows 40 cols 130; exec \"$@\"")
            .arg("harpe-tui-e2e")
            .arg(bin)
            .arg("--addr")
            .arg(addr_for_tui)
            .arg("--user-id")
            .arg(user_id)
            .arg("--config")
            .arg(config_path_for_tui)
            .arg("--session-id")
            .arg(session_id);

        let mut tui = spawn_command(command, Some(15_000))?;
        tui.exp_regex("Iron Coast.*First watch")?;
        tui.send("I lift the latch\r")?;
        tui.flush()?;
        tui.exp_string("opens.")?;
        tui.send_control('q')?;
        tui.exp_eof()?;
        Ok::<(), rexpect::error::Error>(())
    })
    .await
    .unwrap();

    server.abort();
    let _ = std::fs::remove_file(config_path);
    test_result.unwrap();
}

async fn test_store() -> SurrealStore {
    SurrealStore::connect("memory", &format!("tui_test_{}", Uuid::now_v7()), "harpe")
        .await
        .unwrap()
}

async fn spawn_grpc_service(
    service: HarpeGrpc,
) -> (
    String,
    tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(
        Server::builder()
            .add_service(AdminServiceServer::new(service.clone()))
            .add_service(HealthServiceServer::new(service.clone()))
            .add_service(MetricsServiceServer::new(service.clone()))
            .add_service(UserServiceServer::new(service.clone()))
            .add_service(GameServiceServer::new(service.clone()))
            .add_service(SessionServiceServer::new(service.clone()))
            .add_service(MemoryServiceServer::new(service))
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );

    (format!("http://{addr}"), server)
}
