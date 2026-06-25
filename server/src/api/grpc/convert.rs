use tonic::Status;

use crate::domain::{
    BackgroundJob, Character, Event, Game, GameSnapshot, JobKind, JobStatus, Location, MemoryChunk,
    MemoryHit, Message, MessageRole, Session, StorySummary, User, WorldFact,
};
use crate::engine::ContextBuilder;
use crate::llm::ChatRequest;
use crate::observability::MetricsSnapshot as AppMetricsSnapshot;
use crate::pb::{self, ContextMessage};
use crate::{HarpeError, Result};

pub(super) fn status_from_error(error: HarpeError) -> Status {
    match error {
        HarpeError::Validation(message) => Status::invalid_argument(message),
        HarpeError::NotFound(message) => Status::not_found(message),
        HarpeError::PermissionDenied(message) => Status::permission_denied(message),
        HarpeError::Store(message) => Status::internal(message),
        HarpeError::Llm(message) => Status::unavailable(message),
    }
}

pub(super) fn user_to_pb(user: User) -> pb::User {
    pb::User {
        id: user.id,
        display_name: user.display_name,
        created_at: user.created_at.to_rfc3339(),
    }
}

pub(super) fn game_to_pb(game: Game) -> pb::Game {
    pb::Game {
        id: game.id,
        title: game.title,
        system_prompt: game.system_prompt,
        created_at: game.created_at.to_rfc3339(),
        owner_user_id: game.owner_user_id,
    }
}

pub(super) fn session_to_pb(session: Session) -> pb::Session {
    pb::Session {
        id: session.id,
        game_id: session.game_id,
        title: session.title,
        created_at: session.created_at.to_rfc3339(),
    }
}

pub(super) fn message_to_pb(message: Message) -> pb::Message {
    pb::Message {
        id: message.id,
        session_id: message.session_id,
        role: role_to_pb(message.role),
        content: message.content,
        created_at: message.created_at.to_rfc3339(),
    }
}

pub(super) fn summary_to_pb(summary: StorySummary) -> pb::StorySummary {
    pb::StorySummary {
        session_id: summary.session_id,
        content: summary.content,
        updated_at: summary.updated_at.to_rfc3339(),
    }
}

pub(super) fn character_to_pb(character: Character) -> pb::Character {
    pb::Character {
        id: character.id,
        game_id: character.game_id,
        name: character.name,
        description: character.description,
        status: character.status,
        updated_at: character.updated_at.to_rfc3339(),
    }
}

pub(super) fn event_to_pb(event: Event) -> pb::Event {
    pb::Event {
        id: event.id,
        session_id: event.session_id,
        summary: event.summary,
        importance: event.importance,
        created_at: event.created_at.to_rfc3339(),
    }
}

pub(super) fn world_fact_to_pb(fact: WorldFact) -> pb::WorldFact {
    pb::WorldFact {
        id: fact.id,
        game_id: fact.game_id,
        subject: fact.subject,
        predicate: fact.predicate,
        object: fact.object,
        content: fact.content,
        confidence: fact.confidence,
        updated_at: fact.updated_at.to_rfc3339(),
    }
}

pub(super) fn location_to_pb(location: Location) -> pb::Location {
    pb::Location {
        id: location.id,
        game_id: location.game_id,
        name: location.name,
        description: location.description,
        updated_at: location.updated_at.to_rfc3339(),
    }
}

pub(super) fn memory_hit_to_pb(hit: MemoryHit) -> pb::MemoryHit {
    pb::MemoryHit {
        id: hit.chunk.id,
        session_id: hit.chunk.session_id,
        kind: hit.chunk.kind,
        content: hit.chunk.content,
        score: hit.score,
    }
}

pub(super) fn memory_chunk_to_pb(chunk: MemoryChunk) -> pb::MemoryChunk {
    pb::MemoryChunk {
        id: chunk.id,
        session_id: chunk.session_id,
        kind: chunk.kind,
        content: chunk.content,
        embedding: chunk.embedding,
        created_at: chunk.created_at.to_rfc3339(),
    }
}

pub(super) fn background_job_to_pb(job: BackgroundJob) -> pb::BackgroundJobDebug {
    pb::BackgroundJobDebug {
        id: job.id,
        kind: admin_job_kind_to_pb(job.kind),
        status: admin_job_status_to_pb(job.status),
        payload_json: serde_json::to_string(&job.payload)
            .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}")),
        attempts: job.attempts,
        max_attempts: job.max_attempts,
        last_error: job.last_error.unwrap_or_default(),
        run_after: job.run_after.to_rfc3339(),
        created_at: job.created_at.to_rfc3339(),
        updated_at: job.updated_at.to_rfc3339(),
    }
}

pub(super) fn snapshot_to_pb(snapshot: GameSnapshot) -> pb::GameSnapshot {
    pb::GameSnapshot {
        game: Some(game_to_pb(snapshot.game)),
        sessions: snapshot.sessions.into_iter().map(session_to_pb).collect(),
        summaries: snapshot.summaries.into_iter().map(summary_to_pb).collect(),
        characters: snapshot
            .characters
            .into_iter()
            .map(character_to_pb)
            .collect(),
        events: snapshot.events.into_iter().map(event_to_pb).collect(),
        world_facts: snapshot
            .world_facts
            .into_iter()
            .map(world_fact_to_pb)
            .collect(),
        locations: snapshot.locations.into_iter().map(location_to_pb).collect(),
        memory_chunks: snapshot
            .memory_chunks
            .into_iter()
            .map(memory_chunk_to_pb)
            .collect(),
        exported_at: snapshot.exported_at.to_rfc3339(),
    }
}

pub(super) fn metrics_to_pb(snapshot: AppMetricsSnapshot) -> pb::MetricsSnapshot {
    pb::MetricsSnapshot {
        grpc_requests: snapshot.grpc_requests,
        grpc_failures: snapshot.grpc_failures,
        streamed_messages: snapshot.streamed_messages,
        jobs_processed: snapshot.jobs_processed,
        jobs_succeeded: snapshot.jobs_succeeded,
        jobs_retried: snapshot.jobs_retried,
        jobs_failed: snapshot.jobs_failed,
        health_checks: snapshot.health_checks,
        grpc_latency_count: snapshot.grpc_latency_count,
        grpc_latency_sum_ms: snapshot.grpc_latency_sum_ms,
        grpc_latency_buckets: snapshot
            .grpc_latency_buckets
            .into_iter()
            .map(|bucket| pb::HistogramBucket {
                le: bucket.le_label(),
                count: bucket.count,
            })
            .collect(),
        collected_at: snapshot.collected_at.to_rfc3339(),
    }
}

pub(super) fn preview_context_to_pb(
    chat_request: ChatRequest,
    context_builder: &ContextBuilder,
) -> pb::PreviewContextResponse {
    let mut estimated_total = 0_usize;
    let messages = chat_request
        .messages
        .into_iter()
        .map(|message| {
            let estimated = context_builder.estimate_tokens(&message.content);
            estimated_total = estimated_total.saturating_add(estimated);

            ContextMessage {
                role: role_to_pb(message.role),
                content: message.content,
                estimated_tokens: saturating_u32(estimated),
            }
        })
        .collect();

    pb::PreviewContextResponse {
        messages,
        estimated_tokens: saturating_u32(estimated_total),
    }
}

pub(super) fn role_to_pb(role: MessageRole) -> i32 {
    match role {
        MessageRole::System => pb::MessageRole::System as i32,
        MessageRole::User => pb::MessageRole::User as i32,
        MessageRole::Assistant => pb::MessageRole::Assistant as i32,
    }
}

pub(super) fn admin_job_kind_to_pb(kind: JobKind) -> i32 {
    match kind {
        JobKind::UpdateMemoryAfterTurn => pb::AdminJobKind::UpdateMemoryAfterTurn as i32,
    }
}

pub(super) fn admin_job_status_to_pb(status: JobStatus) -> i32 {
    match status {
        JobStatus::Pending => pb::AdminJobStatus::Pending as i32,
        JobStatus::Running => pb::AdminJobStatus::Running as i32,
        JobStatus::Succeeded => pb::AdminJobStatus::Succeeded as i32,
        JobStatus::Failed => pb::AdminJobStatus::Failed as i32,
    }
}

pub(super) fn admin_job_status_filter(status: i32) -> Result<Option<JobStatus>> {
    match pb::AdminJobStatus::try_from(status) {
        Ok(pb::AdminJobStatus::Unspecified) => Ok(None),
        Ok(pb::AdminJobStatus::Pending) => Ok(Some(JobStatus::Pending)),
        Ok(pb::AdminJobStatus::Running) => Ok(Some(JobStatus::Running)),
        Ok(pb::AdminJobStatus::Succeeded) => Ok(Some(JobStatus::Succeeded)),
        Ok(pb::AdminJobStatus::Failed) => Ok(Some(JobStatus::Failed)),
        Err(_) => Err(HarpeError::Validation(format!(
            "unknown admin job status {status}"
        ))),
    }
}

pub(super) fn validate_job_id(job_id: &str) -> Result<()> {
    if job_id.trim().is_empty() {
        return Err(HarpeError::Validation("job id is required".to_owned()));
    }

    Ok(())
}

pub(super) fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
