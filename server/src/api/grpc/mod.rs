use std::sync::Arc;

use chrono::Utc;
use futures_util::StreamExt;
use serde::Serialize;
use tokio::sync::mpsc;
use tonic::Status;

use crate::domain::{Game, Message, MessageRole, NewMessage, Session, new_id};
use crate::engine::{ContextBuilder, ContextInputs};
use crate::jobs::{UpdateMemoryAfterTurnPayload, new_update_memory_job};
use crate::llm::{ChatRequest, LlmClient};
use crate::observability::{AppMetrics, LatencyGuard, SharedMetrics};
use crate::pb::{self, MessageDelta, SendMessageRequest};
use crate::store::HarpeStore;
use crate::{HarpeError, Result};

mod admin_service;
mod convert;
mod game_service;
mod health;
mod health_service;
mod memory_service;
mod metrics_service;
mod ownership;
mod pagination;
mod session_service;
mod user_service;

use ownership::require_owned_session;

#[derive(Clone)]
pub struct HarpeGrpc {
    store: Arc<dyn HarpeStore>,
    llm: Arc<dyn LlmClient>,
    context_builder: ContextBuilder,
    metrics: SharedMetrics,
}

impl HarpeGrpc {
    pub fn new(store: Arc<dyn HarpeStore>, llm: Arc<dyn LlmClient>) -> Self {
        Self {
            store,
            llm,
            context_builder: ContextBuilder::default(),
            metrics: AppMetrics::shared(),
        }
    }

    pub fn with_context_builder(mut self, context_builder: ContextBuilder) -> Self {
        self.context_builder = context_builder;
        self
    }

    pub fn with_metrics(mut self, metrics: SharedMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    fn observe_request(&self, rpc: &'static str) -> LatencyGuard<'_> {
        self.metrics.record_grpc_request();
        tracing::debug!(rpc);
        self.metrics.track_grpc_latency()
    }
}

#[tracing::instrument(skip_all, fields(session_id = %request.session_id, user_id = %user_id))]
async fn run_send_message(
    request: SendMessageRequest,
    user_id: String,
    store: Arc<dyn HarpeStore>,
    llm: Arc<dyn LlmClient>,
    metrics: SharedMetrics,
    context_builder: ContextBuilder,
    tx: mpsc::Sender<std::result::Result<MessageDelta, Status>>,
) -> Result<()> {
    if request.content.trim().is_empty() {
        return Err(HarpeError::Validation(
            "message content is required".to_owned(),
        ));
    }

    let session = require_owned_session(store.as_ref(), &request.session_id, &user_id).await?;
    let game = store.get_game(&session.game_id).await?;

    store
        .append_message(NewMessage {
            id: None,
            session_id: session.id.clone(),
            role: MessageRole::User,
            content: request.content.clone(),
        })
        .await?;

    let model = optional_model_override(&request.model);
    let mut chat_request = build_context_for_turn(
        &session,
        &game,
        &request.content,
        false,
        store.as_ref(),
        llm.as_ref(),
        &context_builder,
    )
    .await?;
    chat_request.model = model;

    let assistant_id = new_id();
    let mut response_stream = llm.stream_chat(chat_request).await?;
    let mut assistant_content = String::new();
    let mut sequence = 0_u32;

    while let Some(delta) = response_stream.next().await {
        let delta = delta?;
        assistant_content.push_str(&delta);
        sequence = sequence.saturating_add(1);

        if tx
            .send(Ok(MessageDelta {
                session_id: session.id.clone(),
                message_id: assistant_id.clone(),
                delta,
                done: false,
                sequence,
                finish_reason: pb::MessageFinishReason::InProgress as i32,
            }))
            .await
            .is_err()
        {
            return Ok(());
        }
        metrics.record_streamed_message();
    }

    if assistant_content.trim().is_empty() {
        return Err(HarpeError::Llm("assistant response was empty".to_owned()));
    }

    store
        .append_message(NewMessage {
            id: Some(assistant_id.clone()),
            session_id: session.id.clone(),
            role: MessageRole::Assistant,
            content: assistant_content.clone(),
        })
        .await?;

    store
        .enqueue_job(new_update_memory_job(UpdateMemoryAfterTurnPayload::new(
            game.id,
            session.id.clone(),
            assistant_id.clone(),
            assistant_content,
        ))?)
        .await?;

    let _ = tx
        .send(Ok(MessageDelta {
            session_id: session.id,
            message_id: assistant_id,
            delta: String::new(),
            done: true,
            sequence: sequence.saturating_add(1),
            finish_reason: pb::MessageFinishReason::AssistantComplete as i32,
        }))
        .await;

    Ok(())
}

fn optional_model_override(model: &str) -> Option<String> {
    let model = model.trim();
    (!model.is_empty()).then(|| model.to_owned())
}

#[tracing::instrument(skip_all, fields(session_id = %session.id, game_id = %game.id))]
async fn build_context_for_turn(
    session: &Session,
    game: &Game,
    user_content: &str,
    include_ephemeral_user_message: bool,
    store: &dyn HarpeStore,
    llm: &dyn LlmClient,
    context_builder: &ContextBuilder,
) -> Result<ChatRequest> {
    let query_embedding = llm.embed(user_content).await?;
    let summary = store.get_story_summary(&session.id).await?;
    let recent_events = store.list_events(&session.id, 12).await?;
    let memories = store
        .search_memory(
            &session.id,
            user_content,
            &query_embedding,
            context_builder.memory_limit,
        )
        .await?;
    let characters = store.list_characters(&game.id).await?;
    let world_facts = store.list_world_facts(&game.id, 24).await?;
    let locations = store.list_locations(&game.id).await?;
    let mut recent_messages = store
        .list_recent_messages(&session.id, context_builder.recent_message_limit)
        .await?;

    if include_ephemeral_user_message {
        recent_messages.push(Message {
            id: String::new(),
            session_id: session.id.clone(),
            role: MessageRole::User,
            content: user_content.to_owned(),
            created_at: Utc::now(),
        });
    }

    Ok(context_builder.build(ContextInputs {
        base_system_prompt: game.system_prompt.clone(),
        summary,
        recent_events,
        memories,
        characters,
        world_facts,
        locations,
        recent_messages,
    }))
}

#[tracing::instrument(skip_all, fields(game_id = %game.id))]
async fn run_export_game_stream(
    game: Game,
    store: Arc<dyn HarpeStore>,
    tx: mpsc::Sender<std::result::Result<pb::GameBackupChunk, Status>>,
) -> Result<()> {
    let game_id = game.id.clone();
    let mut sequence = 0_u32;
    send_backup_chunk(&tx, &game_id, "game", &mut sequence, &game).await?;

    let sessions = store.list_sessions(&game_id, 1_000).await?;
    for session in &sessions {
        send_backup_chunk(&tx, &game_id, "session", &mut sequence, session).await?;
        if let Some(summary) = store.get_story_summary(&session.id).await? {
            send_backup_chunk(&tx, &game_id, "story_summary", &mut sequence, &summary).await?;
        }
        for event in store.list_events(&session.id, 1_000).await? {
            send_backup_chunk(&tx, &game_id, "event", &mut sequence, &event).await?;
        }
        for memory in store.list_memory_chunks(&session.id, 1_000).await? {
            send_backup_chunk(&tx, &game_id, "memory_chunk", &mut sequence, &memory).await?;
        }
    }

    for character in store.list_characters(&game_id).await? {
        send_backup_chunk(&tx, &game_id, "character", &mut sequence, &character).await?;
    }
    for fact in store.list_world_facts(&game_id, 1_000).await? {
        send_backup_chunk(&tx, &game_id, "world_fact", &mut sequence, &fact).await?;
    }
    for location in store.list_locations(&game_id).await? {
        send_backup_chunk(&tx, &game_id, "location", &mut sequence, &location).await?;
    }

    sequence = sequence.saturating_add(1);
    let _ = tx
        .send(Ok(pb::GameBackupChunk {
            game_id,
            kind: "done".to_owned(),
            sequence,
            payload_json: "{}".to_owned(),
            done: true,
        }))
        .await;

    Ok(())
}

async fn send_backup_chunk<T: Serialize>(
    tx: &mpsc::Sender<std::result::Result<pb::GameBackupChunk, Status>>,
    game_id: &str,
    kind: &str,
    sequence: &mut u32,
    payload: &T,
) -> Result<()> {
    *sequence = sequence.saturating_add(1);
    let payload_json =
        serde_json::to_string(payload).map_err(|error| HarpeError::Store(error.to_string()))?;
    if tx
        .send(Ok(pb::GameBackupChunk {
            game_id: game_id.to_owned(),
            kind: kind.to_owned(),
            sequence: *sequence,
            payload_json,
            done: false,
        }))
        .await
        .is_err()
    {
        return Ok(());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use tonic::Request;
    use uuid::Uuid;

    use crate::db::surreal::SurrealStore;
    use crate::domain::{JobKind, JobStatus, NewBackgroundJob};
    use crate::llm::{ChatMessage, EchoLlm};
    use crate::pb::{ExportMetricsRequest, GetMetricsRequest, metrics_service_server};

    use super::convert::{
        admin_job_status_filter, admin_job_status_to_pb, metrics_to_pb, preview_context_to_pb,
        status_from_error,
    };
    use super::health::{health_response, normalize_health_service};
    use super::ownership::resolve_owner_user_id;
    use super::pagination::{page_info, request_limit, truncate_to_limit};

    use super::*;

    #[test]
    fn owner_user_id_uses_metadata_and_rejects_mismatch() {
        assert_eq!(resolve_owner_user_id(Some("user-1"), "").unwrap(), "user-1");
        assert_eq!(
            resolve_owner_user_id(Some("user-1"), "user-1").unwrap(),
            "user-1"
        );
        assert_eq!(resolve_owner_user_id(None, "user-1").unwrap(), "user-1");

        let mismatch = resolve_owner_user_id(Some("user-1"), "user-2").unwrap_err();
        assert!(matches!(mismatch, HarpeError::PermissionDenied(_)));

        let missing = resolve_owner_user_id(None, "").unwrap_err();
        assert!(matches!(missing, HarpeError::PermissionDenied(_)));
    }

    #[test]
    fn preview_context_response_includes_token_estimates() {
        let response = preview_context_to_pb(
            ChatRequest {
                messages: vec![
                    ChatMessage {
                        role: MessageRole::System,
                        content: "Trusted state.".to_owned(),
                    },
                    ChatMessage {
                        role: MessageRole::User,
                        content: "I open the gate.".to_owned(),
                    },
                ],
                model: None,
            },
            &ContextBuilder::default(),
        );

        assert_eq!(response.messages.len(), 2);
        assert!(response.estimated_tokens >= response.messages[0].estimated_tokens);
        assert_eq!(response.messages[1].role, pb::MessageRole::User as i32);
    }

    #[test]
    fn optional_model_override_trims_blank_values() {
        assert_eq!(optional_model_override(""), None);
        assert_eq!(optional_model_override("   "), None);
        assert_eq!(
            optional_model_override(" gpt-5-mini "),
            Some("gpt-5-mini".to_owned())
        );
    }

    #[test]
    fn domain_errors_map_to_expected_grpc_status_codes() {
        let cases = [
            (
                HarpeError::Validation("content".to_owned()),
                tonic::Code::InvalidArgument,
            ),
            (
                HarpeError::NotFound("summary".to_owned()),
                tonic::Code::NotFound,
            ),
            (
                HarpeError::PermissionDenied("game-1".to_owned()),
                tonic::Code::PermissionDenied,
            ),
            (
                HarpeError::Store("database".to_owned()),
                tonic::Code::Internal,
            ),
            (
                HarpeError::Llm("model".to_owned()),
                tonic::Code::Unavailable,
            ),
        ];

        for (error, code) in cases {
            assert_eq!(status_from_error(error).code(), code);
        }
    }

    #[test]
    fn health_service_name_defaults_when_blank() {
        assert_eq!(normalize_health_service(""), "harpe.v1");
        assert_eq!(
            normalize_health_service(" harpe.v1.MemoryService "),
            "harpe.v1.MemoryService"
        );
    }

    #[test]
    fn page_request_overrides_legacy_limit_and_reports_counts() {
        assert_eq!(request_limit(25, None), 25);
        assert_eq!(
            request_limit(
                25,
                Some(&pb::PageRequest {
                    page_size: 3,
                    page_token: String::new(),
                }),
            ),
            3
        );
        assert_eq!(
            request_limit(
                25,
                Some(&pb::PageRequest {
                    page_size: 0,
                    page_token: "ignored-for-now".to_owned(),
                }),
            ),
            25
        );

        let mut items = vec![1, 2, 3, 4];
        truncate_to_limit(&mut items, 2);
        assert_eq!(items, vec![1, 2]);
        assert_eq!(page_info(items.len()).returned_count, 2);
    }

    #[test]
    fn admin_job_status_filter_maps_proto_statuses() {
        assert_eq!(
            admin_job_status_filter(pb::AdminJobStatus::Unspecified as i32).unwrap(),
            None
        );
        assert_eq!(
            admin_job_status_filter(pb::AdminJobStatus::Pending as i32).unwrap(),
            Some(JobStatus::Pending)
        );
        assert_eq!(
            admin_job_status_filter(pb::AdminJobStatus::Running as i32).unwrap(),
            Some(JobStatus::Running)
        );
        assert_eq!(
            admin_job_status_filter(pb::AdminJobStatus::Succeeded as i32).unwrap(),
            Some(JobStatus::Succeeded)
        );
        assert_eq!(
            admin_job_status_filter(pb::AdminJobStatus::Failed as i32).unwrap(),
            Some(JobStatus::Failed)
        );
        assert!(matches!(
            admin_job_status_filter(99),
            Err(HarpeError::Validation(_))
        ));
    }

    #[test]
    fn admin_job_statuses_map_to_proto_values() {
        let cases = [
            (JobStatus::Pending, pb::AdminJobStatus::Pending),
            (JobStatus::Running, pb::AdminJobStatus::Running),
            (JobStatus::Succeeded, pb::AdminJobStatus::Succeeded),
            (JobStatus::Failed, pb::AdminJobStatus::Failed),
        ];

        for (status, proto_status) in cases {
            assert_eq!(admin_job_status_to_pb(status), proto_status as i32);
        }
    }

    #[test]
    fn metrics_snapshot_maps_to_proto() {
        let metrics = AppMetrics::default();
        metrics.record_grpc_request();
        metrics.record_job_retried();

        let response = metrics_to_pb(metrics.snapshot());

        assert_eq!(response.grpc_requests, 1);
        assert_eq!(response.jobs_retried, 1);
        assert!(!response.grpc_latency_buckets.is_empty());
        assert!(!response.collected_at.is_empty());
    }

    #[tokio::test]
    async fn metrics_service_returns_snapshots_and_prometheus_text() {
        let service = test_grpc_service().await;

        let snapshot = metrics_service_server::MetricsService::get_metrics(
            &service,
            Request::new(GetMetricsRequest {}),
        )
        .await
        .unwrap()
        .into_inner();
        assert_eq!(snapshot.grpc_requests, 1);

        let exported = metrics_service_server::MetricsService::export_metrics(
            &service,
            Request::new(ExportMetricsRequest {
                format: pb::MetricsExportFormat::PrometheusText as i32,
            }),
        )
        .await
        .unwrap()
        .into_inner();
        assert_eq!(
            exported.content_type,
            "text/plain; version=0.0.4; charset=utf-8"
        );
        assert!(exported.body.contains("harpe_grpc_requests_total 2"));

        let error = metrics_service_server::MetricsService::export_metrics(
            &service,
            Request::new(ExportMetricsRequest { format: 99 }),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn health_response_reports_failed_jobs_as_degraded() {
        let store = test_store().await;
        store
            .enqueue_job(NewBackgroundJob {
                kind: JobKind::UpdateMemoryAfterTurn,
                payload: serde_json::json!({}),
                max_attempts: 1,
                run_after: None,
            })
            .await
            .unwrap();
        let job = store.claim_next_job().await.unwrap().unwrap();
        store
            .fail_job(&job.id, "permanent failure".to_owned())
            .await
            .unwrap();

        let response = health_response(store.as_ref(), "harpe.v1".to_owned()).await;

        assert_eq!(response.status, pb::ServingStatus::Degraded as i32);
        assert!(response.database_ok);
        assert_eq!(response.pending_jobs, 0);
        assert_eq!(response.failed_jobs, 1);
    }

    async fn test_grpc_service() -> HarpeGrpc {
        HarpeGrpc::new(test_store().await, Arc::new(EchoLlm::development_default()))
    }

    async fn test_store() -> Arc<SurrealStore> {
        Arc::new(
            SurrealStore::connect(
                "memory",
                &format!("grpc_unit_test_{}", Uuid::now_v7()),
                "harpe",
            )
            .await
            .unwrap(),
        )
    }
}
