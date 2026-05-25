use std::sync::Arc;

use futures_util::StreamExt;
use harpe_server::api::grpc::HarpeGrpc;
use harpe_server::db::surreal::SurrealStore;
use harpe_server::domain::{
    ExtractedCharacterUpdate, ExtractedEvent, ExtractedLocation, ExtractedWorldFact,
    GraphRelationKind, JobKind, JobStatus, MemoryExtraction, MessageRole, NewBackgroundJob,
    NewEvent, NewGame, NewMemoryChunk, NewMessage, NewSession, NewUser, UpsertCharacter,
    UpsertLocation, UpsertStorySummary, UpsertWorldFact,
};
use harpe_server::jobs::JobRunner;
use harpe_server::llm::EchoLlm;
use harpe_server::pb::game_service_client::GameServiceClient;
use harpe_server::pb::game_service_server::GameServiceServer;
use harpe_server::pb::memory_service_client::MemoryServiceClient;
use harpe_server::pb::memory_service_server::MemoryServiceServer;
use harpe_server::pb::session_service_client::SessionServiceClient;
use harpe_server::pb::session_service_server::SessionServiceServer;
use harpe_server::pb::user_service_client::UserServiceClient;
use harpe_server::pb::user_service_server::UserServiceServer;
use harpe_server::pb::{
    CreateGameRequest, CreateSessionRequest, CreateUserRequest, GetGameRequest,
    GetStorySummaryRequest, ListMessagesRequest, ListWorldFactsRequest, PreviewContextRequest,
    SearchMemoryRequest, SendMessageRequest,
};
use harpe_server::store::HarpeStore;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::metadata::MetadataValue;
use tonic::transport::{Endpoint, Server};
use tonic::{Code, Request};
use uuid::Uuid;

#[tokio::test]
async fn surreal_store_round_trips_conversation_memory_and_characters() {
    let store = test_store().await;
    store.migrate().await.unwrap();

    let user = store
        .create_user(NewUser {
            display_name: "Noah".to_owned(),
        })
        .await
        .unwrap();
    let game = store
        .create_game(NewGame {
            owner_user_id: user.id.clone(),
            title: "Vaults of Glass".to_owned(),
            system_prompt: "Run a tense fantasy mystery.".to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(game.owner_user_id, user.id);
    assert_eq!(
        store.list_games_for_user(&user.id, 10).await.unwrap(),
        vec![game.clone()]
    );
    let session = store
        .create_session(NewSession {
            game_id: game.id.clone(),
            title: "Session 1".to_owned(),
        })
        .await
        .unwrap();

    store
        .append_message(NewMessage {
            id: None,
            session_id: session.id.clone(),
            role: MessageRole::User,
            content: "I inspect the silver door.".to_owned(),
        })
        .await
        .unwrap();
    store
        .append_message(NewMessage {
            id: None,
            session_id: session.id.clone(),
            role: MessageRole::Assistant,
            content: "A keyhole glows beneath the dust.".to_owned(),
        })
        .await
        .unwrap();

    let messages = store.list_recent_messages(&session.id, 10).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, MessageRole::User);
    assert_eq!(messages[1].role, MessageRole::Assistant);
    assert_relation_targets(
        &store,
        GraphRelationKind::SessionInGame,
        &session.id,
        "game",
        &game.id,
    )
    .await;
    assert_relation_targets(
        &store,
        GraphRelationKind::MessageInSession,
        &messages[0].id,
        "session",
        &session.id,
    )
    .await;

    store
        .upsert_story_summary(UpsertStorySummary {
            session_id: session.id.clone(),
            content: "The party found a sealed silver door.".to_owned(),
        })
        .await
        .unwrap();
    let summary = store.get_story_summary(&session.id).await.unwrap().unwrap();
    assert!(summary.content.contains("silver door"));

    let character = store
        .upsert_character(UpsertCharacter {
            id: None,
            game_id: game.id.clone(),
            name: "Mira".to_owned(),
            description: "Archivist and reluctant guide".to_owned(),
            status: "nervous".to_owned(),
        })
        .await
        .unwrap();
    let characters = store.list_characters(&game.id).await.unwrap();
    assert_eq!(characters, vec![character]);
    assert_relation_targets(
        &store,
        GraphRelationKind::CharacterInGame,
        &characters[0].id,
        "game",
        &game.id,
    )
    .await;

    let event = store
        .save_event(NewEvent {
            session_id: session.id.clone(),
            summary: "Mira found the vault stairs.".to_owned(),
            importance: 4,
        })
        .await
        .unwrap();
    let events = store.list_events(&session.id, 10).await.unwrap();
    assert_eq!(events, vec![event]);
    assert_relation_targets(
        &store,
        GraphRelationKind::EventInSession,
        &events[0].id,
        "session",
        &session.id,
    )
    .await;

    let location = store
        .upsert_location(UpsertLocation {
            id: None,
            game_id: game.id.clone(),
            name: "Lower Vault".to_owned(),
            description: "A sealed chamber beneath the archive".to_owned(),
        })
        .await
        .unwrap();
    let locations = store.list_locations(&game.id).await.unwrap();
    assert_eq!(locations, vec![location]);
    assert_relation_targets(
        &store,
        GraphRelationKind::LocationInGame,
        &locations[0].id,
        "game",
        &game.id,
    )
    .await;

    let fact = store
        .upsert_world_fact(UpsertWorldFact {
            id: None,
            game_id: game.id.clone(),
            subject: "silver key".to_owned(),
            predicate: "opens".to_owned(),
            object: "lower vault".to_owned(),
            content: String::new(),
            confidence: 1.4,
        })
        .await
        .unwrap();
    let facts = store.list_world_facts(&game.id, 10).await.unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].id, fact.id);
    assert_eq!(facts[0].content, "silver key opens lower vault");
    assert_eq!(facts[0].confidence, 1.0);
    assert_relation_targets(
        &store,
        GraphRelationKind::WorldFactInGame,
        &facts[0].id,
        "game",
        &game.id,
    )
    .await;

    let saved = store
        .save_memory_chunk(NewMemoryChunk {
            session_id: session.id.clone(),
            kind: "event".to_owned(),
            content: "The silver key opens the lower vault.".to_owned(),
            embedding: vec![1.0, 0.0],
        })
        .await
        .unwrap();
    let hits = store
        .search_memory(&session.id, "silver key", &[1.0, 0.0], 5)
        .await
        .unwrap();

    assert_eq!(hits[0].chunk.id, saved.chunk.id);
    assert!(hits[0].score > 0.99);
    assert_relation_targets(
        &store,
        GraphRelationKind::MemoryInSession,
        &saved.chunk.id,
        "session",
        &session.id,
    )
    .await;
}

#[tokio::test]
async fn surreal_migrations_are_versioned_and_idempotent() {
    let store = test_store().await;

    let first_run = store.applied_migrations().await.unwrap();
    assert_eq!(
        first_run
            .iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>(),
        SurrealStore::migration_versions()
    );

    store.migrate().await.unwrap();
    store.migrate().await.unwrap();
    let second_run = store.applied_migrations().await.unwrap();

    assert_eq!(second_run.len(), first_run.len());
    assert_eq!(
        second_run
            .iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>(),
        SurrealStore::migration_versions()
    );
}

#[tokio::test]
async fn surreal_store_claims_completes_and_fails_background_jobs() {
    let store = test_store().await;

    let job = store
        .enqueue_job(NewBackgroundJob {
            kind: JobKind::UpdateMemoryAfterTurn,
            payload: serde_json::json!({"session_id": "session-1"}),
            max_attempts: 0,
            run_after: None,
        })
        .await
        .unwrap();
    assert_eq!(job.status, JobStatus::Pending);
    assert_eq!(job.max_attempts, 1);

    let pending = store.list_jobs(Some(JobStatus::Pending), 10).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, job.id);

    let claimed = store.claim_next_job().await.unwrap().unwrap();
    assert_eq!(claimed.id, job.id);
    assert_eq!(claimed.status, JobStatus::Running);
    assert_eq!(claimed.attempts, 1);

    let completed = store.complete_job(&claimed.id).await.unwrap();
    assert_eq!(completed.status, JobStatus::Succeeded);
    assert!(store.claim_next_job().await.unwrap().is_none());

    store
        .enqueue_job(NewBackgroundJob {
            kind: JobKind::UpdateMemoryAfterTurn,
            payload: serde_json::json!({"session_id": "session-2"}),
            max_attempts: 3,
            run_after: None,
        })
        .await
        .unwrap();
    let failing_job = store.claim_next_job().await.unwrap().unwrap();
    let failed = store
        .fail_job(&failing_job.id, "model timeout".to_owned())
        .await
        .unwrap();

    assert_eq!(failed.id, failing_job.id);
    assert_eq!(failed.status, JobStatus::Failed);
    assert_eq!(failed.last_error.as_deref(), Some("model timeout"));
}

#[tokio::test]
async fn grpc_send_message_streams_response_and_updates_memory() {
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
                    subject: "sea gate".to_owned(),
                    predicate: "guards".to_owned(),
                    object: "Iron Coast harbor".to_owned(),
                    content: "The sea gate guards Iron Coast harbor.".to_owned(),
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
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(
        Server::builder()
            .add_service(UserServiceServer::new(service.clone()))
            .add_service(GameServiceServer::new(service.clone()))
            .add_service(SessionServiceServer::new(service.clone()))
            .add_service(MemoryServiceServer::new(service))
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );

    let channel = Endpoint::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();

    let mut user_client = UserServiceClient::new(channel.clone());
    let user = user_client
        .create_user(CreateUserRequest {
            display_name: "Noah".to_owned(),
        })
        .await
        .unwrap()
        .into_inner();

    let mut game_client = GameServiceClient::new(channel.clone());
    let game = game_client
        .create_game(with_user(
            CreateGameRequest {
                title: "Iron Coast".to_owned(),
                system_prompt: "Run a coastal fantasy adventure.".to_owned(),
                owner_user_id: String::new(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(game.owner_user_id, user.id);

    let stranger = user_client
        .create_user(CreateUserRequest {
            display_name: "Kest".to_owned(),
        })
        .await
        .unwrap()
        .into_inner();
    let denied = game_client
        .get_game(with_user(
            GetGameRequest {
                game_id: game.id.clone(),
            },
            &stranger.id,
        ))
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);

    let mut session_client = SessionServiceClient::new(channel.clone());
    let session = session_client
        .create_session(with_user(
            CreateSessionRequest {
                game_id: game.id.clone(),
                title: "First watch".to_owned(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner();

    let preview = session_client
        .preview_context(with_user(
            PreviewContextRequest {
                session_id: session.id.clone(),
                content: "I lift the rusted latch.".to_owned(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(preview.estimated_tokens > 0);
    assert!(
        preview
            .messages
            .iter()
            .any(|message| message.content.contains("I lift the rusted latch."))
    );

    let mut stream = session_client
        .send_message(with_user(
            SendMessageRequest {
                session_id: session.id.clone(),
                content: "I lift the rusted latch.".to_owned(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner();

    let mut response = String::new();
    let mut saw_done = false;
    while let Some(delta) = stream.next().await {
        let delta = delta.unwrap();
        response.push_str(&delta.delta);
        saw_done = delta.done;
        if saw_done {
            break;
        }
    }

    assert_eq!(response, "The gate opens.");
    assert!(saw_done);

    let messages = session_client
        .list_messages(with_user(
            ListMessagesRequest {
                session_id: session.id.clone(),
                limit: 10,
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner()
        .messages;
    assert_eq!(messages.len(), 2);

    assert!(
        store
            .get_story_summary(&session.id)
            .await
            .unwrap()
            .is_none()
    );
    let jobs = store.list_jobs(Some(JobStatus::Pending), 10).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].kind, JobKind::UpdateMemoryAfterTurn);

    let processed = JobRunner::new(store.clone(), llm)
        .process_all_pending_jobs(10)
        .await
        .unwrap();
    assert_eq!(processed, 1);

    let mut memory_client = MemoryServiceClient::new(channel);
    let summary = memory_client
        .get_story_summary(with_user(
            GetStorySummaryRequest {
                session_id: session.id.clone(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(summary.content.contains("The gate opens."));

    let events = memory_client
        .list_events(with_user(
            harpe_server::pb::ListEventsRequest {
                session_id: summary.session_id.clone(),
                limit: 10,
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner()
        .events;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].summary, "The rusted sea gate opens.");

    let characters = memory_client
        .list_characters(with_user(
            harpe_server::pb::ListCharactersRequest {
                game_id: game.id.clone(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner()
        .characters;
    assert_eq!(characters.len(), 1);
    assert_eq!(characters[0].name, "Mira");
    assert_eq!(characters[0].status, "alert");

    let facts = memory_client
        .list_world_facts(with_user(
            ListWorldFactsRequest {
                game_id: game.id.clone(),
                limit: 10,
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner()
        .facts;
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].content, "The sea gate guards Iron Coast harbor.");

    let locations = memory_client
        .list_locations(with_user(
            harpe_server::pb::ListLocationsRequest {
                game_id: game.id.clone(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner()
        .locations;
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].name, "Iron Coast harbor");

    let hits = memory_client
        .search_memory(with_user(
            SearchMemoryRequest {
                session_id: summary.session_id,
                query: "sea gate harbor".to_owned(),
                limit: 10,
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner()
        .hits;
    assert!(hits.iter().any(|hit| hit.kind == "world_fact"));

    server.abort();
}

async fn test_store() -> SurrealStore {
    SurrealStore::connect("memory", &format!("test_{}", Uuid::now_v7()), "harpe")
        .await
        .unwrap()
}

fn with_user<T>(message: T, user_id: &str) -> Request<T> {
    let mut request = Request::new(message);
    request.metadata_mut().insert(
        "x-user-id",
        MetadataValue::try_from(user_id).expect("test user id is valid metadata"),
    );
    request
}

async fn assert_relation_targets(
    store: &SurrealStore,
    relation: GraphRelationKind,
    in_id: &str,
    out_table: &str,
    out_id: &str,
) {
    let edges = store.list_graph_edges(relation, in_id).await.unwrap();

    assert_eq!(edges.len(), 1);
    assert!(
        edges[0].out_record.starts_with(&format!("{out_table}:")),
        "unexpected out record: {}",
        edges[0].out_record
    );
    assert!(
        edges[0].out_record.contains(out_id),
        "unexpected out record: {}",
        edges[0].out_record
    );
}
