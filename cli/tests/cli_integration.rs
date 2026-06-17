use std::sync::Arc;

use clap::Parser;
use harpe_cli::{Cli, execute};
use harpe_server::api::grpc::HarpeGrpc;
use harpe_server::db::surreal::SurrealStore;
use harpe_server::domain::{
    ExtractedCharacterUpdate, ExtractedEvent, ExtractedLocation, ExtractedWorldFact, JobKind,
    MemoryExtraction, NewBackgroundJob,
};
use harpe_server::jobs::JobRunner;
use harpe_server::llm::EchoLlm;
use harpe_server::pb::admin_service_server::AdminServiceServer;
use harpe_server::pb::game_service_server::GameServiceServer;
use harpe_server::pb::health_service_server::HealthServiceServer;
use harpe_server::pb::memory_service_server::MemoryServiceServer;
use harpe_server::pb::metrics_service_server::MetricsServiceServer;
use harpe_server::pb::session_service_server::SessionServiceServer;
use harpe_server::pb::user_service_server::UserServiceServer;
use harpe_server::store::HarpeStore;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use uuid::Uuid;

#[tokio::test]
async fn cli_drives_core_roleplay_flow_against_real_grpc_server() {
    let store = Arc::new(test_store().await);
    let llm = Arc::new(
        EchoLlm::new(vec!["The gate ".to_owned(), "opens.".to_owned()]).with_extraction(
            MemoryExtraction {
                events: vec![ExtractedEvent {
                    summary: "The rusted sea gate opens.".to_owned(),
                    importance: 4,
                }],
                character_updates: vec![ExtractedCharacterUpdate {
                    name: "Mira".to_owned(),
                    description: "A scout watching the gate".to_owned(),
                    status: "alert".to_owned(),
                }],
                world_facts: vec![ExtractedWorldFact {
                    subject: "Mira".to_owned(),
                    predicate: "watches".to_owned(),
                    object: "sea gate".to_owned(),
                    content: "Mira watches the sea gate at Iron Coast harbor.".to_owned(),
                    confidence: 0.9,
                }],
                locations: vec![ExtractedLocation {
                    name: "Iron Coast harbor".to_owned(),
                    description: "A storm-battered harbor behind a rusted gate".to_owned(),
                }],
            },
        ),
    );
    let service = HarpeGrpc::new(store.clone(), llm.clone());
    let (addr, server) = spawn_grpc_service(service).await;

    let health = run_json(&addr, None, &["health"]).await;
    assert_eq!(health["status"], "serving");
    assert_eq!(health["database_ok"], true);

    let user = run_json(&addr, None, &["user", "create", "--name", "Noah"]).await;
    let user_id = user["id"].as_str().unwrap().to_owned();
    assert_eq!(user["display_name"], "Noah");

    let game = run_json(
        &addr,
        Some(&user_id),
        &[
            "game",
            "create",
            "--title",
            "Iron Coast",
            "--system-prompt",
            "Run a coastal fantasy adventure.",
        ],
    )
    .await;
    let game_id = game["id"].as_str().unwrap().to_owned();
    assert_eq!(game["owner_user_id"], user_id);

    let games = run_json(&addr, Some(&user_id), &["game", "list", "--limit", "10"]).await;
    assert_eq!(games["games"].as_array().unwrap().len(), 1);
    assert_eq!(games["games"][0]["id"], game_id);

    let fetched_game = run_json(&addr, Some(&user_id), &["game", "get", &game_id]).await;
    assert_eq!(fetched_game["id"], game_id);

    let session = run_json(
        &addr,
        Some(&user_id),
        &[
            "session",
            "create",
            "--game",
            &game_id,
            "--title",
            "First watch",
        ],
    )
    .await;
    let session_id = session["id"].as_str().unwrap().to_owned();
    assert_eq!(session["game_id"], game_id);

    let sessions = run_json(
        &addr,
        Some(&user_id),
        &["session", "list", "--game", &game_id, "--limit", "10"],
    )
    .await;
    assert_eq!(sessions["sessions"].as_array().unwrap().len(), 1);

    let fetched_session = run_json(&addr, Some(&user_id), &["session", "get", &session_id]).await;
    assert_eq!(fetched_session["id"], session_id);

    let context = run_json(
        &addr,
        Some(&user_id),
        &[
            "session",
            "context",
            &session_id,
            "I",
            "lift",
            "the",
            "latch",
        ],
    )
    .await;
    assert!(context["estimated_tokens"].as_u64().unwrap() > 0);
    assert!(
        context["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"]
                .as_str()
                .unwrap()
                .contains("I lift the latch"))
    );

    let send = run_json(
        &addr,
        Some(&user_id),
        &["session", "send", &session_id, "I", "lift", "the", "latch"],
    )
    .await;
    assert_eq!(send["response"], "The gate opens.");
    assert_eq!(send["deltas"].as_array().unwrap().len(), 3);
    assert_eq!(send["deltas"][2]["finish_reason"], "assistant_complete");

    let messages = run_json(
        &addr,
        Some(&user_id),
        &["session", "messages", &session_id, "--limit", "10"],
    )
    .await;
    assert_eq!(messages["messages"].as_array().unwrap().len(), 2);
    assert_eq!(messages["messages"][0]["role"], "user");
    assert_eq!(messages["messages"][1]["role"], "assistant");

    let second_session = run_json(
        &addr,
        Some(&user_id),
        &[
            "session",
            "create",
            "--game",
            &game_id,
            "--title",
            "Second watch",
        ],
    )
    .await;
    let second_session_id = second_session["id"].as_str().unwrap().to_owned();
    let text_response = run_text(
        &addr,
        Some(&user_id),
        &["session", "send", &second_session_id, "I", "wait"],
    )
    .await;
    assert_eq!(text_response.trim(), "The gate opens.");

    let processed = JobRunner::new(store.clone(), llm)
        .process_all_pending_jobs(10)
        .await
        .unwrap();
    assert_eq!(processed, 2);

    let summary = run_json(&addr, Some(&user_id), &["memory", "summary", &session_id]).await;
    assert!(
        summary["content"]
            .as_str()
            .unwrap()
            .contains("The gate opens.")
    );

    let characters = run_json(
        &addr,
        Some(&user_id),
        &["memory", "characters", "--game", &game_id, "--limit", "10"],
    )
    .await;
    assert_eq!(characters["characters"].as_array().unwrap().len(), 1);
    let character_id = characters["characters"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let character = run_json(
        &addr,
        Some(&user_id),
        &["memory", "character", &character_id],
    )
    .await;
    assert_eq!(character["name"], "Mira");
    assert_eq!(character["status"], "alert");

    let events = run_json(
        &addr,
        Some(&user_id),
        &["memory", "events", &session_id, "--limit", "10"],
    )
    .await;
    assert_eq!(events["events"].as_array().unwrap().len(), 1);
    assert_eq!(events["events"][0]["summary"], "The rusted sea gate opens.");

    let facts = run_json(
        &addr,
        Some(&user_id),
        &["memory", "facts", "--game", &game_id, "--limit", "10"],
    )
    .await;
    assert_eq!(facts["facts"].as_array().unwrap().len(), 1);
    assert_eq!(
        facts["facts"][0]["content"],
        "Mira watches the sea gate at Iron Coast harbor."
    );

    let locations = run_json(
        &addr,
        Some(&user_id),
        &["memory", "locations", "--game", &game_id, "--limit", "10"],
    )
    .await;
    assert_eq!(locations["locations"].as_array().unwrap().len(), 1);
    assert_eq!(locations["locations"][0]["name"], "Iron Coast harbor");

    let hits = run_json(
        &addr,
        Some(&user_id),
        &[
            "memory",
            "search",
            &session_id,
            "sea",
            "gate",
            "--limit",
            "10",
        ],
    )
    .await;
    assert!(hits["hits"].as_array().unwrap().iter().any(
        |hit| hit["kind"] == "world_fact" || hit["content"].as_str().unwrap().contains("gate")
    ));

    let backup = run_json(
        &addr,
        Some(&user_id),
        &["backup", "export", "--game", &game_id],
    )
    .await;
    assert_eq!(backup["game"]["id"], game_id);
    assert_eq!(backup["sessions"].as_array().unwrap().len(), 2);
    assert!(!backup["memory_chunks"].as_array().unwrap().is_empty());

    let backup_stream = run_text(
        &addr,
        Some(&user_id),
        &["backup", "stream", "--game", &game_id],
    )
    .await;
    let streamed_chunks = backup_stream
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(streamed_chunks[0]["kind"], "game");
    assert_eq!(streamed_chunks.last().unwrap()["done"], true);

    let memory_chunks = run_json(
        &addr,
        None,
        &["admin", "memory-chunks", &session_id, "--limit", "10"],
    )
    .await;
    assert!(!memory_chunks["chunks"].as_array().unwrap().is_empty());

    let jobs = run_json(
        &addr,
        None,
        &["admin", "jobs", "--status", "succeeded", "--limit", "10"],
    )
    .await;
    assert_eq!(jobs["jobs"].as_array().unwrap().len(), 2);
    assert_eq!(jobs["jobs"][0]["status"], "succeeded");

    let retry_target = failed_job(&store, "retry-target").await;
    let retried = run_json(
        &addr,
        None,
        &["admin", "retry-job", &retry_target, "--max-attempts", "4"],
    )
    .await;
    assert_eq!(retried["status"], "pending");
    assert_eq!(retried["max_attempts"], 4);

    let purge_target = failed_job(&store, "purge-target").await;
    let purged = run_json(&addr, None, &["admin", "purge-job", &purge_target]).await;
    assert_eq!(purged["status"], "failed");

    let metrics = run_json(&addr, None, &["metrics"]).await;
    assert!(metrics["grpc_requests"].as_u64().unwrap() >= 10);
    assert!(metrics["grpc_latency_count"].as_u64().unwrap() > 0);

    let metrics_export = run_text(&addr, None, &["metrics", "export"]).await;
    assert!(metrics_export.contains("harpe_grpc_requests_total"));

    server.abort();
}

async fn run_json(addr: &str, user_id: Option<&str>, args: &[&str]) -> Value {
    let output = run_cli(addr, user_id, true, args).await;
    serde_json::from_slice(&output).unwrap()
}

async fn run_text(addr: &str, user_id: Option<&str>, args: &[&str]) -> String {
    String::from_utf8(run_cli(addr, user_id, false, args).await).unwrap()
}

async fn run_cli(addr: &str, user_id: Option<&str>, as_json: bool, args: &[&str]) -> Vec<u8> {
    let mut argv = vec!["harpe".to_owned(), "--addr".to_owned(), addr.to_owned()];
    if as_json {
        argv.push("--json".to_owned());
    }
    if let Some(user_id) = user_id {
        argv.push("--user-id".to_owned());
        argv.push(user_id.to_owned());
    }
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));

    let cli = Cli::parse_from(argv);
    let mut output = Vec::new();
    execute(cli, &mut output).await.unwrap();

    output
}

async fn test_store() -> SurrealStore {
    SurrealStore::connect("memory", &format!("cli_test_{}", Uuid::now_v7()), "harpe")
        .await
        .unwrap()
}

async fn failed_job(store: &SurrealStore, session_id: &str) -> String {
    store
        .enqueue_job(NewBackgroundJob {
            kind: JobKind::UpdateMemoryAfterTurn,
            payload: serde_json::json!({ "session_id": session_id }),
            max_attempts: 1,
            run_after: None,
        })
        .await
        .unwrap();
    let job = store.claim_next_job().await.unwrap().unwrap();
    let failed = store
        .fail_job(&job.id, format!("failed {session_id}"))
        .await
        .unwrap();

    failed.id
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
