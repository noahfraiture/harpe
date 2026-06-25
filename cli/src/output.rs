use std::io::Write;
use std::path::Path;

use harpe_proto::pb::{
    self, BackgroundJobDebug, Character, ContextMessage, Event, Game, GameBackupChunk,
    GameSnapshot, HealthCheckResponse, HistogramBucket, Location, MemoryChunk, MemoryHit, Message,
    MessageDelta, MetricsSnapshot, PageInfo, Session, StorySummary, User, WorldFact,
};
use serde_json::{Value, json};

use crate::{CliResult, ClientConfig};

pub(crate) fn write_user<W: Write>(writer: &mut W, as_json: bool, user: &User) -> CliResult<()> {
    if as_json {
        write_json(writer, &user_json(user))
    } else {
        writeln!(
            writer,
            "{}\t{}\tcreated_at={}",
            user.id, user.display_name, user.created_at
        )?;
        Ok(())
    }
}

pub(crate) fn write_game<W: Write>(writer: &mut W, as_json: bool, game: &Game) -> CliResult<()> {
    if as_json {
        write_json(writer, &game_json(game))
    } else {
        writeln!(
            writer,
            "{}\t{}\towner={}\tcreated_at={}",
            game.id, game.title, game.owner_user_id, game.created_at
        )?;
        Ok(())
    }
}

pub(crate) fn write_session<W: Write>(
    writer: &mut W,
    as_json: bool,
    session: &Session,
) -> CliResult<()> {
    if as_json {
        write_json(writer, &session_json(session))
    } else {
        writeln!(
            writer,
            "{}\t{}\tgame={}\tcreated_at={}",
            session.id, session.title, session.game_id, session.created_at
        )?;
        Ok(())
    }
}

pub(crate) fn write_job<W: Write>(
    writer: &mut W,
    as_json: bool,
    job: &BackgroundJobDebug,
) -> CliResult<()> {
    if as_json {
        write_json(writer, &background_job_json(job))
    } else {
        writeln!(
            writer,
            "{}\t{}\tstatus={}\tattempts={}/{}",
            job.id,
            job_kind_name(job.kind),
            admin_status_name(job.status),
            job.attempts,
            job.max_attempts
        )?;
        Ok(())
    }
}

pub(crate) fn write_path_result<W: Write>(
    writer: &mut W,
    as_json: bool,
    key: &str,
    path: &Path,
) -> CliResult<()> {
    if as_json {
        write_json(writer, &json!({ key: path.display().to_string() }))
    } else {
        writeln!(writer, "{}={}", key, path.display())?;
        Ok(())
    }
}

pub(crate) fn write_json<W: Write>(writer: &mut W, value: &Value) -> CliResult<()> {
    serde_json::to_writer_pretty(&mut *writer, value)?;
    writeln!(writer)?;
    Ok(())
}

pub(crate) fn config_json(client_config: &ClientConfig, config_path: &Path) -> Value {
    json!({
        "path": config_path.display().to_string(),
        "addr": client_config.addr,
        "user_id": client_config.user_id,
        "game_id": client_config.game_id,
        "session_id": client_config.session_id,
    })
}

pub(crate) fn user_json(user: &User) -> Value {
    json!({
        "id": user.id,
        "display_name": user.display_name,
        "created_at": user.created_at,
    })
}

pub(crate) fn game_json(game: &Game) -> Value {
    json!({
        "id": game.id,
        "title": game.title,
        "system_prompt": game.system_prompt,
        "created_at": game.created_at,
        "owner_user_id": game.owner_user_id,
    })
}

pub(crate) fn session_json(session: &Session) -> Value {
    json!({
        "id": session.id,
        "game_id": session.game_id,
        "title": session.title,
        "created_at": session.created_at,
    })
}

pub(crate) fn message_json(message: &Message) -> Value {
    json!({
        "id": message.id,
        "session_id": message.session_id,
        "role": role_name(message.role),
        "content": message.content,
        "created_at": message.created_at,
    })
}

pub(crate) fn context_message_json(message: &ContextMessage) -> Value {
    json!({
        "role": role_name(message.role),
        "content": message.content,
        "estimated_tokens": message.estimated_tokens,
    })
}

pub(crate) fn delta_json(delta: &MessageDelta) -> Value {
    json!({
        "session_id": delta.session_id,
        "message_id": delta.message_id,
        "delta": delta.delta,
        "done": delta.done,
        "sequence": delta.sequence,
        "finish_reason": finish_reason_name(delta.finish_reason),
    })
}

pub(crate) fn story_summary_json(summary: &StorySummary) -> Value {
    json!({
        "session_id": summary.session_id,
        "content": summary.content,
        "updated_at": summary.updated_at,
    })
}

pub(crate) fn character_json(character: &Character) -> Value {
    json!({
        "id": character.id,
        "game_id": character.game_id,
        "name": character.name,
        "description": character.description,
        "status": character.status,
        "updated_at": character.updated_at,
    })
}

pub(crate) fn event_json(event: &Event) -> Value {
    json!({
        "id": event.id,
        "session_id": event.session_id,
        "summary": event.summary,
        "importance": event.importance,
        "created_at": event.created_at,
    })
}

pub(crate) fn world_fact_json(fact: &WorldFact) -> Value {
    json!({
        "id": fact.id,
        "game_id": fact.game_id,
        "subject": fact.subject,
        "predicate": fact.predicate,
        "object": fact.object,
        "content": fact.content,
        "confidence": fact.confidence,
        "updated_at": fact.updated_at,
    })
}

pub(crate) fn location_json(location: &Location) -> Value {
    json!({
        "id": location.id,
        "game_id": location.game_id,
        "name": location.name,
        "description": location.description,
        "updated_at": location.updated_at,
    })
}

pub(crate) fn memory_hit_json(hit: &MemoryHit) -> Value {
    json!({
        "id": hit.id,
        "session_id": hit.session_id,
        "kind": hit.kind,
        "content": hit.content,
        "score": hit.score,
    })
}

pub(crate) fn memory_chunk_json(chunk: &MemoryChunk) -> Value {
    json!({
        "id": chunk.id,
        "session_id": chunk.session_id,
        "kind": chunk.kind,
        "content": chunk.content,
        "embedding_dims": chunk.embedding.len(),
        "embedding": chunk.embedding,
        "created_at": chunk.created_at,
    })
}

pub(crate) fn health_json(health: &HealthCheckResponse) -> Value {
    json!({
        "status": serving_status_name(health.status),
        "service": health.service,
        "version": health.version,
        "database_ok": health.database_ok,
        "pending_jobs": health.pending_jobs,
        "failed_jobs": health.failed_jobs,
        "checked_at": health.checked_at,
    })
}

pub(crate) fn metrics_json(metrics: &MetricsSnapshot) -> Value {
    json!({
        "grpc_requests": metrics.grpc_requests,
        "grpc_failures": metrics.grpc_failures,
        "streamed_messages": metrics.streamed_messages,
        "jobs_processed": metrics.jobs_processed,
        "jobs_succeeded": metrics.jobs_succeeded,
        "jobs_retried": metrics.jobs_retried,
        "jobs_failed": metrics.jobs_failed,
        "health_checks": metrics.health_checks,
        "collected_at": metrics.collected_at,
        "grpc_latency_count": metrics.grpc_latency_count,
        "grpc_latency_sum_ms": metrics.grpc_latency_sum_ms,
        "grpc_latency_buckets": metrics.grpc_latency_buckets.iter().map(histogram_bucket_json).collect::<Vec<_>>(),
    })
}

fn histogram_bucket_json(bucket: &HistogramBucket) -> Value {
    json!({
        "le": bucket.le,
        "count": bucket.count,
    })
}

pub(crate) fn background_job_json(job: &BackgroundJobDebug) -> Value {
    json!({
        "id": job.id,
        "kind": job_kind_name(job.kind),
        "status": admin_status_name(job.status),
        "payload_json": job.payload_json,
        "attempts": job.attempts,
        "max_attempts": job.max_attempts,
        "last_error": job.last_error,
        "run_after": job.run_after,
        "created_at": job.created_at,
        "updated_at": job.updated_at,
    })
}

pub(crate) fn game_snapshot_json(snapshot: &GameSnapshot) -> Value {
    json!({
        "game": snapshot.game.as_ref().map(game_json),
        "sessions": snapshot.sessions.iter().map(session_json).collect::<Vec<_>>(),
        "summaries": snapshot.summaries.iter().map(story_summary_json).collect::<Vec<_>>(),
        "characters": snapshot.characters.iter().map(character_json).collect::<Vec<_>>(),
        "events": snapshot.events.iter().map(event_json).collect::<Vec<_>>(),
        "world_facts": snapshot.world_facts.iter().map(world_fact_json).collect::<Vec<_>>(),
        "locations": snapshot.locations.iter().map(location_json).collect::<Vec<_>>(),
        "memory_chunks": snapshot.memory_chunks.iter().map(memory_chunk_json).collect::<Vec<_>>(),
        "exported_at": snapshot.exported_at,
    })
}

pub(crate) fn backup_chunk_json(chunk: &GameBackupChunk) -> Value {
    let payload = serde_json::from_str::<Value>(&chunk.payload_json)
        .unwrap_or_else(|_| json!({ "raw": chunk.payload_json }));
    json!({
        "game_id": chunk.game_id,
        "kind": chunk.kind,
        "sequence": chunk.sequence,
        "payload": payload,
        "done": chunk.done,
    })
}

pub(crate) fn page_json(page: Option<&PageInfo>) -> Value {
    match page {
        Some(page) => json!({
            "next_page_token": page.next_page_token,
            "returned_count": page.returned_count,
        }),
        None => Value::Null,
    }
}

pub(crate) fn role_name(role: i32) -> &'static str {
    match pb::MessageRole::try_from(role).ok() {
        Some(pb::MessageRole::System) => "system",
        Some(pb::MessageRole::User) => "user",
        Some(pb::MessageRole::Assistant) => "assistant",
        Some(pb::MessageRole::Unspecified) | None => "unspecified",
    }
}

pub(crate) fn finish_reason_name(reason: i32) -> &'static str {
    match pb::MessageFinishReason::try_from(reason).ok() {
        Some(pb::MessageFinishReason::InProgress) => "in_progress",
        Some(pb::MessageFinishReason::AssistantComplete) => "assistant_complete",
        Some(pb::MessageFinishReason::Unspecified) | None => "unspecified",
    }
}

pub(crate) fn serving_status_name(status: i32) -> &'static str {
    match pb::ServingStatus::try_from(status).ok() {
        Some(pb::ServingStatus::Serving) => "serving",
        Some(pb::ServingStatus::Degraded) => "degraded",
        Some(pb::ServingStatus::NotServing) => "not_serving",
        Some(pb::ServingStatus::Unspecified) | None => "unspecified",
    }
}

pub(crate) fn admin_status_name(status: i32) -> &'static str {
    match pb::AdminJobStatus::try_from(status).ok() {
        Some(pb::AdminJobStatus::Pending) => "pending",
        Some(pb::AdminJobStatus::Running) => "running",
        Some(pb::AdminJobStatus::Succeeded) => "succeeded",
        Some(pb::AdminJobStatus::Failed) => "failed",
        Some(pb::AdminJobStatus::Unspecified) | None => "unspecified",
    }
}

pub(crate) fn job_kind_name(kind: i32) -> &'static str {
    match pb::AdminJobKind::try_from(kind).ok() {
        Some(pb::AdminJobKind::UpdateMemoryAfterTurn) => "update_memory_after_turn",
        Some(pb::AdminJobKind::Unspecified) | None => "unspecified",
    }
}
