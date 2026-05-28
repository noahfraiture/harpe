use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use chrono::Utc;
use futures_util::{StreamExt, stream::iter};
use harpe_server::HarpeError;
use harpe_server::api::grpc::HarpeGrpc;
use harpe_server::db::surreal::SurrealStore;
use harpe_server::domain::{
    ExtractedCharacterUpdate, ExtractedEvent, ExtractedLocation, ExtractedWorldFact,
    GraphRelationKind, JobKind, JobStatus, MemoryExtraction, MessageRole, NewBackgroundJob,
    NewEvent, NewGame, NewMemoryChunk, NewMessage, NewSession, NewUser, UpsertCharacter,
    UpsertLocation, UpsertStorySummary, UpsertWorldFact,
};
use harpe_server::jobs::{JobRunner, UpdateMemoryAfterTurnPayload, update_memory_after_turn};
use harpe_server::llm::{
    ChatRequest, EchoLlm, ExtractMemoryRequest, LlmClient, SummarizeRequest, TextStream,
};
use harpe_server::observability::AppMetrics;
use harpe_server::pb::game_service_client::GameServiceClient;
use harpe_server::pb::game_service_server::GameServiceServer;
use harpe_server::pb::health_service_client::HealthServiceClient;
use harpe_server::pb::health_service_server::HealthServiceServer;
use harpe_server::pb::memory_service_client::MemoryServiceClient;
use harpe_server::pb::memory_service_server::MemoryServiceServer;
use harpe_server::pb::metrics_service_client::MetricsServiceClient;
use harpe_server::pb::metrics_service_server::MetricsServiceServer;
use harpe_server::pb::session_service_client::SessionServiceClient;
use harpe_server::pb::session_service_server::SessionServiceServer;
use harpe_server::pb::user_service_client::UserServiceClient;
use harpe_server::pb::user_service_server::UserServiceServer;
use harpe_server::pb::{
    CreateGameRequest, CreateSessionRequest, CreateUserRequest, ExportGameRequest, GetGameRequest,
    GetMetricsRequest, GetStorySummaryRequest, HealthCheckRequest, ListGamesRequest,
    ListMessagesRequest, ListWorldFactsRequest, PreviewContextRequest, SearchMemoryRequest,
    SendMessageRequest,
};
use harpe_server::store::HarpeStore;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint, Server};
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

    let relevant_old_memory = store
        .save_memory_chunk(NewMemoryChunk {
            session_id: session.id.clone(),
            kind: "lore".to_owned(),
            content: "The violet comet unlocks the oldest seal.".to_owned(),
            embedding: vec![0.0; 16],
        })
        .await
        .unwrap();
    for index in 0..250 {
        store
            .save_memory_chunk(NewMemoryChunk {
                session_id: session.id.clone(),
                kind: "noise".to_owned(),
                content: format!("Routine camp note {index}."),
                embedding: vec![0.0; 16],
            })
            .await
            .unwrap();
    }
    let indexed_hits = store
        .search_memory(&session.id, "violet comet", &[0.0; 16], 5)
        .await
        .unwrap();
    assert_eq!(indexed_hits[0].chunk.id, relevant_old_memory.chunk.id);

    let chunks = store.list_memory_chunks(&session.id, 10).await.unwrap();
    assert_eq!(chunks.len(), 10);
    assert_eq!(chunks[0].id, saved.chunk.id);

    let snapshot = store.export_game_snapshot(&game.id).await.unwrap();
    assert_eq!(snapshot.game.id, game.id);
    assert_eq!(snapshot.sessions, vec![session.clone()]);
    assert_eq!(snapshot.summaries.len(), 1);
    assert_eq!(snapshot.characters.len(), 1);
    assert_eq!(snapshot.events.len(), 1);
    assert_eq!(snapshot.world_facts.len(), 1);
    assert_eq!(snapshot.locations.len(), 1);
    assert_eq!(snapshot.memory_chunks.len(), 252);
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
async fn surreal_store_rejects_invalid_inputs_and_reports_not_found() {
    let store = test_store().await;

    assert_validation(
        store
            .create_user(NewUser {
                display_name: " ".to_owned(),
            })
            .await
            .unwrap_err(),
        "display name",
    );
    assert_not_found(
        store.get_user("missing-user").await.unwrap_err(),
        "missing-user",
    );
    assert_validation(
        store
            .create_game(NewGame {
                owner_user_id: " ".to_owned(),
                title: "Invalid".to_owned(),
                system_prompt: String::new(),
            })
            .await
            .unwrap_err(),
        "owner user id",
    );

    let user = store
        .create_user(NewUser {
            display_name: "Noah".to_owned(),
        })
        .await
        .unwrap();
    assert_not_found(
        store
            .create_game(NewGame {
                owner_user_id: "missing-owner".to_owned(),
                title: "Invalid owner".to_owned(),
                system_prompt: String::new(),
            })
            .await
            .unwrap_err(),
        "missing-owner",
    );
    let game = store
        .create_game(NewGame {
            owner_user_id: user.id,
            title: "Validation Coast".to_owned(),
            system_prompt: String::new(),
        })
        .await
        .unwrap();

    assert_validation(
        store
            .create_session(NewSession {
                game_id: " ".to_owned(),
                title: "Invalid".to_owned(),
            })
            .await
            .unwrap_err(),
        "game id",
    );
    assert_not_found(
        store
            .create_session(NewSession {
                game_id: "missing-game".to_owned(),
                title: "Invalid".to_owned(),
            })
            .await
            .unwrap_err(),
        "missing-game",
    );
    assert_not_found(
        store.get_session("missing-session").await.unwrap_err(),
        "missing-session",
    );
    let session = store
        .create_session(NewSession {
            game_id: game.id.clone(),
            title: "Validation session".to_owned(),
        })
        .await
        .unwrap();

    assert_validation(
        store
            .append_message(NewMessage {
                id: None,
                session_id: session.id.clone(),
                role: MessageRole::User,
                content: " ".to_owned(),
            })
            .await
            .unwrap_err(),
        "message content",
    );
    assert_validation(
        store
            .upsert_story_summary(UpsertStorySummary {
                session_id: " ".to_owned(),
                content: "Invalid".to_owned(),
            })
            .await
            .unwrap_err(),
        "session id",
    );
    assert_validation(
        store
            .upsert_character(UpsertCharacter {
                id: None,
                game_id: game.id.clone(),
                name: " ".to_owned(),
                description: String::new(),
                status: String::new(),
            })
            .await
            .unwrap_err(),
        "character name",
    );
    assert_not_found(
        store.get_character("missing-character").await.unwrap_err(),
        "missing-character",
    );
    assert_validation(
        store
            .save_event(NewEvent {
                session_id: session.id.clone(),
                summary: " ".to_owned(),
                importance: 3,
            })
            .await
            .unwrap_err(),
        "event summary",
    );
    assert_validation(
        store
            .upsert_location(UpsertLocation {
                id: None,
                game_id: game.id.clone(),
                name: " ".to_owned(),
                description: String::new(),
            })
            .await
            .unwrap_err(),
        "location name",
    );
    assert_validation(
        store
            .upsert_world_fact(UpsertWorldFact {
                id: None,
                game_id: game.id.clone(),
                subject: "beacon".to_owned(),
                predicate: "marks".to_owned(),
                object: " ".to_owned(),
                content: String::new(),
                confidence: 0.8,
            })
            .await
            .unwrap_err(),
        "world fact object",
    );
    assert_validation(
        store
            .save_memory_chunk(NewMemoryChunk {
                session_id: session.id,
                kind: "event".to_owned(),
                content: " ".to_owned(),
                embedding: vec![0.0; 16],
            })
            .await
            .unwrap_err(),
        "memory content",
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

    let future_job = store
        .enqueue_job(NewBackgroundJob {
            kind: JobKind::UpdateMemoryAfterTurn,
            payload: serde_json::json!({"session_id": "session-future"}),
            max_attempts: 3,
            run_after: Some(Utc::now() + chrono::Duration::seconds(60)),
        })
        .await
        .unwrap();
    assert!(store.claim_next_job().await.unwrap().is_none());
    let retried = store
        .retry_job(
            &future_job.id,
            "retry soon".to_owned(),
            Utc::now() - chrono::Duration::seconds(1),
        )
        .await
        .unwrap();
    assert_eq!(retried.status, JobStatus::Pending);
    assert_eq!(retried.last_error.as_deref(), Some("retry soon"));
    let claimed_retry = store.claim_next_job().await.unwrap().unwrap();
    assert_eq!(claimed_retry.id, future_job.id);
    store.complete_job(&claimed_retry.id).await.unwrap();

    store
        .enqueue_job(NewBackgroundJob {
            kind: JobKind::UpdateMemoryAfterTurn,
            payload: serde_json::json!({"session_id": "session-2"}),
            max_attempts: 3,
            run_after: Some(Utc::now() - chrono::Duration::seconds(1)),
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
async fn job_runner_retries_transient_memory_update_failures() {
    let store = Arc::new(test_store().await);
    let user = store
        .create_user(NewUser {
            display_name: "Noah".to_owned(),
        })
        .await
        .unwrap();
    let game = store
        .create_game(NewGame {
            owner_user_id: user.id,
            title: "Retry Coast".to_owned(),
            system_prompt: "Run a retry test.".to_owned(),
        })
        .await
        .unwrap();
    let session = store
        .create_session(NewSession {
            game_id: game.id.clone(),
            title: "Retry session".to_owned(),
        })
        .await
        .unwrap();
    store
        .append_message(NewMessage {
            id: None,
            session_id: session.id.clone(),
            role: MessageRole::User,
            content: "I test the retry path.".to_owned(),
        })
        .await
        .unwrap();
    let assistant = store
        .append_message(NewMessage {
            id: None,
            session_id: session.id.clone(),
            role: MessageRole::Assistant,
            content: "The retry beacon flashes.".to_owned(),
        })
        .await
        .unwrap();
    let job = store
        .enqueue_job(NewBackgroundJob {
            kind: JobKind::UpdateMemoryAfterTurn,
            payload: UpdateMemoryAfterTurnPayload::new(
                game.id.clone(),
                session.id.clone(),
                assistant.id,
                "The retry beacon flashes.".to_owned(),
            )
            .into_value()
            .unwrap(),
            max_attempts: 2,
            run_after: None,
        })
        .await
        .unwrap();
    let metrics = AppMetrics::shared();
    let runner = JobRunner::new(store.clone(), Arc::new(FlakySummarizeLlm::new(1)))
        .with_metrics(metrics.clone());

    assert_eq!(runner.process_all_pending_jobs(10).await.unwrap(), 1);
    let pending = store.list_jobs(Some(JobStatus::Pending), 10).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, job.id);
    assert_eq!(pending[0].attempts, 1);
    assert!(
        pending[0]
            .last_error
            .as_deref()
            .unwrap()
            .contains("transient")
    );
    assert!(pending[0].run_after > Utc::now());

    store
        .retry_job(
            &job.id,
            "ready now".to_owned(),
            Utc::now() - chrono::Duration::seconds(1),
        )
        .await
        .unwrap();
    assert_eq!(runner.process_all_pending_jobs(10).await.unwrap(), 1);

    let succeeded = store
        .list_jobs(Some(JobStatus::Succeeded), 10)
        .await
        .unwrap();
    assert_eq!(succeeded.len(), 1);
    assert_eq!(succeeded[0].attempts, 2);
    let summary = store.get_story_summary(&session.id).await.unwrap().unwrap();
    assert!(summary.content.contains("Recovered summary"));

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.jobs_processed, 2);
    assert_eq!(snapshot.jobs_retried, 1);
    assert_eq!(snapshot.jobs_succeeded, 1);
    assert_eq!(snapshot.jobs_failed, 0);
}

#[tokio::test]
async fn job_runner_marks_exhausted_jobs_failed() {
    let store = Arc::new(test_store().await);
    let user = store
        .create_user(NewUser {
            display_name: "Noah".to_owned(),
        })
        .await
        .unwrap();
    let game = store
        .create_game(NewGame {
            owner_user_id: user.id,
            title: "Failure Coast".to_owned(),
            system_prompt: "Run a failure test.".to_owned(),
        })
        .await
        .unwrap();
    let session = store
        .create_session(NewSession {
            game_id: game.id.clone(),
            title: "Failure session".to_owned(),
        })
        .await
        .unwrap();
    let assistant = store
        .append_message(NewMessage {
            id: None,
            session_id: session.id.clone(),
            role: MessageRole::Assistant,
            content: "The beacon burns out.".to_owned(),
        })
        .await
        .unwrap();
    let job = store
        .enqueue_job(NewBackgroundJob {
            kind: JobKind::UpdateMemoryAfterTurn,
            payload: UpdateMemoryAfterTurnPayload::new(
                game.id,
                session.id,
                assistant.id,
                "The beacon burns out.".to_owned(),
            )
            .into_value()
            .unwrap(),
            max_attempts: 1,
            run_after: None,
        })
        .await
        .unwrap();
    let metrics = AppMetrics::shared();
    let runner = JobRunner::new(store.clone(), Arc::new(FlakySummarizeLlm::new(1)))
        .with_metrics(metrics.clone());

    let error = runner.process_all_pending_jobs(10).await.unwrap_err();
    assert!(error.to_string().contains("transient"));

    let failed = store.list_jobs(Some(JobStatus::Failed), 10).await.unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].id, job.id);
    assert_eq!(failed[0].attempts, 1);
    assert!(
        failed[0]
            .last_error
            .as_deref()
            .unwrap()
            .contains("transient")
    );

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.jobs_processed, 1);
    assert_eq!(snapshot.jobs_retried, 0);
    assert_eq!(snapshot.jobs_succeeded, 0);
    assert_eq!(snapshot.jobs_failed, 1);
}

#[tokio::test]
async fn job_runner_fails_jobs_that_target_a_different_game_than_the_session() {
    let store = Arc::new(test_store().await);
    let user = store
        .create_user(NewUser {
            display_name: "Noah".to_owned(),
        })
        .await
        .unwrap();
    let first_game = store
        .create_game(NewGame {
            owner_user_id: user.id.clone(),
            title: "First Coast".to_owned(),
            system_prompt: String::new(),
        })
        .await
        .unwrap();
    let second_game = store
        .create_game(NewGame {
            owner_user_id: user.id,
            title: "Second Coast".to_owned(),
            system_prompt: String::new(),
        })
        .await
        .unwrap();
    let session = store
        .create_session(NewSession {
            game_id: first_game.id.clone(),
            title: "Mismatched session".to_owned(),
        })
        .await
        .unwrap();
    let assistant = store
        .append_message(NewMessage {
            id: None,
            session_id: session.id.clone(),
            role: MessageRole::Assistant,
            content: "The path points elsewhere.".to_owned(),
        })
        .await
        .unwrap();
    let job = store
        .enqueue_job(NewBackgroundJob {
            kind: JobKind::UpdateMemoryAfterTurn,
            payload: UpdateMemoryAfterTurnPayload::new(
                second_game.id,
                session.id,
                assistant.id,
                "The path points elsewhere.".to_owned(),
            )
            .into_value()
            .unwrap(),
            max_attempts: 1,
            run_after: None,
        })
        .await
        .unwrap();
    let runner = JobRunner::new(store.clone(), Arc::new(EchoLlm::development_default()));

    let error = runner.process_all_pending_jobs(10).await.unwrap_err();
    assert!(error.to_string().contains("targets game"));

    let failed = store.list_jobs(Some(JobStatus::Failed), 10).await.unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].id, job.id);
    assert!(
        failed[0]
            .last_error
            .as_deref()
            .unwrap()
            .contains("targets game")
    );
}

#[tokio::test]
async fn memory_update_ignores_blank_extracted_items_without_creating_records() {
    let store = test_store().await;
    let user = store
        .create_user(NewUser {
            display_name: "Noah".to_owned(),
        })
        .await
        .unwrap();
    let game = store
        .create_game(NewGame {
            owner_user_id: user.id,
            title: "Blank Memory Coast".to_owned(),
            system_prompt: String::new(),
        })
        .await
        .unwrap();
    let session = store
        .create_session(NewSession {
            game_id: game.id.clone(),
            title: "Blank extraction session".to_owned(),
        })
        .await
        .unwrap();
    store
        .append_message(NewMessage {
            id: None,
            session_id: session.id.clone(),
            role: MessageRole::User,
            content: "I wait for the empty signal.".to_owned(),
        })
        .await
        .unwrap();
    store
        .append_message(NewMessage {
            id: None,
            session_id: session.id.clone(),
            role: MessageRole::Assistant,
            content: "The empty signal fades.".to_owned(),
        })
        .await
        .unwrap();
    let llm = EchoLlm::new(Vec::new()).with_extraction(MemoryExtraction {
        events: vec![ExtractedEvent {
            summary: " ".to_owned(),
            importance: 4,
        }],
        character_updates: vec![ExtractedCharacterUpdate {
            name: " ".to_owned(),
            description: "Should be ignored".to_owned(),
            status: "ignored".to_owned(),
        }],
        world_facts: vec![ExtractedWorldFact {
            subject: " ".to_owned(),
            predicate: "marks".to_owned(),
            object: "coast".to_owned(),
            content: "Should be ignored".to_owned(),
            confidence: 0.7,
        }],
        locations: vec![ExtractedLocation {
            name: " ".to_owned(),
            description: "Should be ignored".to_owned(),
        }],
    });

    update_memory_after_turn(&session, &game.id, "The empty signal fades.", &store, &llm)
        .await
        .unwrap();

    assert!(
        store
            .get_story_summary(&session.id)
            .await
            .unwrap()
            .unwrap()
            .content
            .contains("The empty signal fades.")
    );
    assert!(store.list_events(&session.id, 10).await.unwrap().is_empty());
    assert!(store.list_characters(&game.id).await.unwrap().is_empty());
    assert!(
        store
            .list_world_facts(&game.id, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(store.list_locations(&game.id).await.unwrap().is_empty());
    let chunks = store.list_memory_chunks(&session.id, 10).await.unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].kind, "turn");
    assert_eq!(chunks[0].content, "The empty signal fades.");
}

#[tokio::test]
async fn grpc_send_message_stream_reports_validation_and_empty_assistant_errors() {
    let store = Arc::new(test_store().await);
    let metrics = AppMetrics::shared();
    let service =
        HarpeGrpc::new(store.clone(), Arc::new(EmptyAssistantLlm)).with_metrics(metrics.clone());
    let (channel, server) = spawn_grpc_service(service).await;

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
                title: "Error Coast".to_owned(),
                system_prompt: String::new(),
                owner_user_id: String::new(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner();
    let mut session_client = SessionServiceClient::new(channel.clone());
    let session = session_client
        .create_session(with_user(
            CreateSessionRequest {
                game_id: game.id,
                title: "Error session".to_owned(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner();

    let mut empty_content_stream = session_client
        .send_message(with_user(
            SendMessageRequest {
                session_id: session.id.clone(),
                content: " ".to_owned(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner();
    let empty_content_error = empty_content_stream.next().await.unwrap().unwrap_err();
    assert_eq!(empty_content_error.code(), Code::InvalidArgument);
    assert!(empty_content_error.message().contains("message content"));

    let mut empty_assistant_stream = session_client
        .send_message(with_user(
            SendMessageRequest {
                session_id: session.id.clone(),
                content: "I wait for an answer.".to_owned(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner();
    let empty_assistant_error = empty_assistant_stream.next().await.unwrap().unwrap_err();
    assert_eq!(empty_assistant_error.code(), Code::Unavailable);
    assert!(
        empty_assistant_error
            .message()
            .contains("assistant response was empty")
    );

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
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, harpe_server::pb::MessageRole::User as i32);
    assert!(
        store
            .list_jobs(Some(JobStatus::Pending), 10)
            .await
            .unwrap()
            .is_empty()
    );

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.grpc_failures, 2);
    assert_eq!(snapshot.streamed_messages, 0);

    server.abort();
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
    let metrics = AppMetrics::shared();
    let service = HarpeGrpc::new(store.clone(), llm.clone()).with_metrics(metrics.clone());
    let (channel, server) = spawn_grpc_service(service).await;

    let health = HealthServiceClient::new(channel.clone())
        .check(HealthCheckRequest {
            service: String::new(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        health.status,
        harpe_server::pb::ServingStatus::Serving as i32
    );
    assert!(health.database_ok);

    store
        .enqueue_job(NewBackgroundJob {
            kind: JobKind::UpdateMemoryAfterTurn,
            payload: serde_json::json!({"session_id": "failed-health-check-job"}),
            max_attempts: 3,
            run_after: None,
        })
        .await
        .unwrap();
    let failed_job = store.claim_next_job().await.unwrap().unwrap();
    store
        .fail_job(&failed_job.id, "test failure".to_owned())
        .await
        .unwrap();
    let degraded_health = HealthServiceClient::new(channel.clone())
        .check(HealthCheckRequest {
            service: "harpe.v1.MemoryService".to_owned(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        degraded_health.status,
        harpe_server::pb::ServingStatus::Degraded as i32
    );
    assert_eq!(degraded_health.failed_jobs, 1);
    assert_eq!(degraded_health.service, "harpe.v1.MemoryService");

    let mut user_client = UserServiceClient::new(channel.clone());
    let user = user_client
        .create_user(CreateUserRequest {
            display_name: "Noah".to_owned(),
        })
        .await
        .unwrap()
        .into_inner();
    let fetched_user = user_client
        .get_user(harpe_server::pb::GetUserRequest {
            user_id: user.id.clone(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(fetched_user.id, user.id);

    let mut game_client = GameServiceClient::new(channel.clone());
    let missing_auth = game_client
        .list_games(ListGamesRequest { limit: 10 })
        .await
        .unwrap_err();
    assert_eq!(missing_auth.code(), Code::PermissionDenied);

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
    let fetched_session = session_client
        .get_session(with_user(
            harpe_server::pb::GetSessionRequest {
                session_id: session.id.clone(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(fetched_session.id, session.id);

    let missing_summary =
        memory_client_get_story_summary_before_update(channel.clone(), &session.id, &user.id).await;
    assert_eq!(missing_summary.code(), Code::NotFound);

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
        .with_metrics(metrics.clone())
        .process_all_pending_jobs(10)
        .await
        .unwrap();
    assert_eq!(processed, 1);

    let mut memory_client = MemoryServiceClient::new(channel.clone());
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
    let character = memory_client
        .get_character(with_user(
            harpe_server::pb::GetCharacterRequest {
                character_id: characters[0].id.clone(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(character.name, "Mira");

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

    let snapshot = memory_client
        .export_game(with_user(
            ExportGameRequest {
                game_id: game.id.clone(),
            },
            &user.id,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(snapshot.game.unwrap().id, game.id);
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(snapshot.memory_chunks.len(), 5);

    let metrics_snapshot = MetricsServiceClient::new(channel)
        .get_metrics(GetMetricsRequest {})
        .await
        .unwrap()
        .into_inner();
    assert!(metrics_snapshot.grpc_requests >= 15);
    assert_eq!(metrics_snapshot.health_checks, 2);
    assert_eq!(metrics_snapshot.streamed_messages, 2);
    assert_eq!(metrics_snapshot.jobs_processed, 1);
    assert_eq!(metrics_snapshot.jobs_succeeded, 1);
    assert_eq!(metrics_snapshot.jobs_retried, 0);
    assert_eq!(metrics_snapshot.jobs_failed, 0);

    server.abort();
}

async fn test_store() -> SurrealStore {
    SurrealStore::connect("memory", &format!("test_{}", Uuid::now_v7()), "harpe")
        .await
        .unwrap()
}

async fn spawn_grpc_service(
    service: HarpeGrpc,
) -> (
    Channel,
    tokio::task::JoinHandle<std::result::Result<(), tonic::transport::Error>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(
        Server::builder()
            .add_service(HealthServiceServer::new(service.clone()))
            .add_service(MetricsServiceServer::new(service.clone()))
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

    (channel, server)
}

fn with_user<T>(message: T, user_id: &str) -> Request<T> {
    let mut request = Request::new(message);
    request.metadata_mut().insert(
        "x-user-id",
        MetadataValue::try_from(user_id).expect("test user id is valid metadata"),
    );
    request
}

fn assert_validation(error: HarpeError, expected_message: &str) {
    match error {
        HarpeError::Validation(message) => assert!(
            message.contains(expected_message),
            "expected validation message to contain {expected_message:?}, got {message:?}"
        ),
        other => panic!("expected validation error, got {other:?}"),
    }
}

fn assert_not_found(error: HarpeError, expected_message: &str) {
    match error {
        HarpeError::NotFound(message) => assert!(
            message.contains(expected_message),
            "expected not found message to contain {expected_message:?}, got {message:?}"
        ),
        other => panic!("expected not found error, got {other:?}"),
    }
}

async fn memory_client_get_story_summary_before_update(
    channel: Channel,
    session_id: &str,
    user_id: &str,
) -> tonic::Status {
    MemoryServiceClient::new(channel)
        .get_story_summary(with_user(
            GetStorySummaryRequest {
                session_id: session_id.to_owned(),
            },
            user_id,
        ))
        .await
        .unwrap_err()
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

struct EmptyAssistantLlm;

#[async_trait]
impl LlmClient for EmptyAssistantLlm {
    async fn stream_chat(&self, _request: ChatRequest) -> harpe_server::Result<TextStream> {
        Ok(Box::pin(iter(Vec::<harpe_server::Result<String>>::new())))
    }

    async fn summarize(&self, _request: SummarizeRequest) -> harpe_server::Result<String> {
        Ok("Empty assistant test summary.".to_owned())
    }

    async fn extract_memory(
        &self,
        _request: ExtractMemoryRequest,
    ) -> harpe_server::Result<MemoryExtraction> {
        Ok(MemoryExtraction::default())
    }

    async fn embed(&self, _text: &str) -> harpe_server::Result<Vec<f32>> {
        Ok(vec![0.0; 16])
    }
}

struct FlakySummarizeLlm {
    failures_remaining: AtomicUsize,
}

impl FlakySummarizeLlm {
    fn new(failures: usize) -> Self {
        Self {
            failures_remaining: AtomicUsize::new(failures),
        }
    }
}

#[async_trait]
impl LlmClient for FlakySummarizeLlm {
    async fn stream_chat(&self, _request: ChatRequest) -> harpe_server::Result<TextStream> {
        Ok(Box::pin(tokio_stream::iter(vec![Ok(
            "flaky response".to_owned()
        )])))
    }

    async fn summarize(&self, _request: SummarizeRequest) -> harpe_server::Result<String> {
        if self.failures_remaining.load(Ordering::SeqCst) > 0 {
            self.failures_remaining.fetch_sub(1, Ordering::SeqCst);
            return Err(harpe_server::HarpeError::Llm(
                "transient summarize failure".to_owned(),
            ));
        }

        Ok("Recovered summary after retry.".to_owned())
    }

    async fn extract_memory(
        &self,
        _request: ExtractMemoryRequest,
    ) -> harpe_server::Result<MemoryExtraction> {
        Ok(MemoryExtraction {
            events: vec![ExtractedEvent {
                summary: "The retry beacon flashes.".to_owned(),
                importance: 3,
            }],
            ..MemoryExtraction::default()
        })
    }

    async fn embed(&self, _text: &str) -> harpe_server::Result<Vec<f32>> {
        Ok(vec![1.0; 16])
    }
}
