use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{
    BackgroundJob, JobKind, JobStatus, MemoryExtraction, NewBackgroundJob, NewEvent,
    NewMemoryChunk, Session, UpsertCharacter, UpsertLocation, UpsertStorySummary, UpsertWorldFact,
    WorldFact,
};
use crate::llm::{ExtractMemoryRequest, LlmClient, SummarizeRequest};
use crate::observability::{AppMetrics, SharedMetrics};
use crate::store::HarpeStore;
use crate::{HarpeError, Result};

#[derive(Clone)]
pub struct JobRunner {
    store: Arc<dyn HarpeStore>,
    llm: Arc<dyn LlmClient>,
    metrics: SharedMetrics,
}

impl JobRunner {
    pub fn new(store: Arc<dyn HarpeStore>, llm: Arc<dyn LlmClient>) -> Self {
        Self {
            store,
            llm,
            metrics: AppMetrics::shared(),
        }
    }

    pub fn with_metrics(mut self, metrics: SharedMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    pub async fn process_next_job(&self) -> Result<Option<BackgroundJob>> {
        let Some(job) = self.store.claim_next_job().await? else {
            return Ok(None);
        };
        self.metrics.record_job_processed();

        if let Err(error) = self.process_claimed_job(&job).await {
            return self.handle_failed_job(&job, error).await.map(Some);
        }

        let completed = self.store.complete_job(&job.id).await?;
        self.metrics.record_job_succeeded();
        tracing::info!(job_id = %job.id, attempts = job.attempts, "background job succeeded");

        Ok(Some(completed))
    }

    pub async fn process_all_pending_jobs(&self, limit: usize) -> Result<usize> {
        let mut processed = 0;
        let limit = limit.max(1);

        while processed < limit {
            if self.process_next_job().await?.is_none() {
                break;
            }
            processed += 1;
        }

        Ok(processed)
    }

    pub fn spawn(self, interval: Duration, batch_limit: usize) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                if let Err(error) = self.process_all_pending_jobs(batch_limit).await {
                    tracing::warn!(%error, "background job batch failed");
                }

                tokio::time::sleep(interval).await;
            }
        })
    }

    async fn process_claimed_job(&self, job: &BackgroundJob) -> Result<()> {
        if job.status != JobStatus::Running {
            return Err(HarpeError::Validation(format!(
                "background job {} is not running",
                job.id
            )));
        }

        match job.kind {
            JobKind::UpdateMemoryAfterTurn => {
                let payload = UpdateMemoryAfterTurnPayload::from_value(job.payload.clone())?;
                let session = self.store.get_session(&payload.session_id).await?;
                if session.game_id != payload.game_id {
                    return Err(HarpeError::Validation(format!(
                        "job {} targets game {} but session {} belongs to {}",
                        job.id, payload.game_id, session.id, session.game_id
                    )));
                }

                update_memory_after_turn(
                    &session,
                    &payload.game_id,
                    &payload.assistant_content,
                    self.store.as_ref(),
                    self.llm.as_ref(),
                )
                .await
            }
        }
    }

    async fn handle_failed_job(
        &self,
        job: &BackgroundJob,
        error: HarpeError,
    ) -> Result<BackgroundJob> {
        if should_retry(job) {
            let delay = retry_delay_for_attempt(job.attempts);
            let run_after = Utc::now()
                + chrono::Duration::from_std(delay)
                    .map_err(|error| HarpeError::Store(error.to_string()))?;
            let retried = self
                .store
                .retry_job(&job.id, error.to_string(), run_after)
                .await?;
            self.metrics.record_job_retried();
            tracing::warn!(
                job_id = %job.id,
                attempts = job.attempts,
                max_attempts = job.max_attempts,
                retry_after_ms = delay.as_millis(),
                error = %error,
                "background job scheduled for retry"
            );

            return Ok(retried);
        }

        self.store.fail_job(&job.id, error.to_string()).await?;
        self.metrics.record_job_failed();
        tracing::error!(
            job_id = %job.id,
            attempts = job.attempts,
            max_attempts = job.max_attempts,
            error = %error,
            "background job permanently failed"
        );

        Err(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateMemoryAfterTurnPayload {
    pub game_id: String,
    pub session_id: String,
    pub assistant_message_id: String,
    pub assistant_content: String,
}

impl UpdateMemoryAfterTurnPayload {
    pub fn new(
        game_id: String,
        session_id: String,
        assistant_message_id: String,
        assistant_content: String,
    ) -> Self {
        Self {
            game_id,
            session_id,
            assistant_message_id,
            assistant_content,
        }
    }

    pub fn into_value(self) -> Result<Value> {
        serde_json::to_value(self).map_err(|error| HarpeError::Store(error.to_string()))
    }

    pub fn from_value(value: Value) -> Result<Self> {
        serde_json::from_value(value).map_err(|error| HarpeError::Validation(error.to_string()))
    }
}

pub fn new_update_memory_job(payload: UpdateMemoryAfterTurnPayload) -> Result<NewBackgroundJob> {
    Ok(NewBackgroundJob {
        kind: JobKind::UpdateMemoryAfterTurn,
        payload: payload.into_value()?,
        max_attempts: 3,
        run_after: None,
    })
}

fn should_retry(job: &BackgroundJob) -> bool {
    job.attempts < job.max_attempts
}

fn retry_delay_for_attempt(attempts: i32) -> Duration {
    let exponent = attempts.clamp(0, 8) as u32;
    Duration::from_secs(2_u64.pow(exponent)).min(Duration::from_secs(300))
}

pub async fn update_memory_after_turn(
    session: &Session,
    game_id: &str,
    assistant_content: &str,
    store: &dyn HarpeStore,
    llm: &dyn LlmClient,
) -> Result<()> {
    let previous_summary = store
        .get_story_summary(&session.id)
        .await?
        .map(|summary| summary.content);
    let recent_messages = store.list_recent_messages(&session.id, 24).await?;
    let updated_summary = llm
        .summarize(SummarizeRequest {
            previous_summary,
            messages: recent_messages.clone(),
        })
        .await?;

    store
        .upsert_story_summary(UpsertStorySummary {
            session_id: session.id.clone(),
            content: updated_summary,
        })
        .await?;

    let embedding = llm.embed(assistant_content).await?;
    store
        .save_memory_chunk(NewMemoryChunk {
            session_id: session.id.clone(),
            kind: "turn".to_owned(),
            content: assistant_content.to_owned(),
            embedding,
        })
        .await?;

    let extraction = llm
        .extract_memory(ExtractMemoryRequest {
            game_id: game_id.to_owned(),
            session_id: session.id.clone(),
            messages: recent_messages,
        })
        .await?;
    persist_extraction(session, game_id, extraction, store, llm).await
}

async fn persist_extraction(
    session: &Session,
    game_id: &str,
    extraction: MemoryExtraction,
    store: &dyn HarpeStore,
    llm: &dyn LlmClient,
) -> Result<()> {
    for event in extraction.events {
        if event.summary.trim().is_empty() {
            continue;
        }

        let event = store
            .save_event(NewEvent {
                session_id: session.id.clone(),
                summary: event.summary,
                importance: event.importance,
            })
            .await?;
        save_embedded_memory(session, "event", event.summary.as_str(), store, llm).await?;
    }

    let existing_characters = store.list_characters(game_id).await?;
    for character in extraction.character_updates {
        if character.name.trim().is_empty() {
            continue;
        }

        let existing_id = existing_characters
            .iter()
            .find(|existing| same_name(&existing.name, &character.name))
            .map(|existing| existing.id.clone());
        let character = store
            .upsert_character(UpsertCharacter {
                id: existing_id,
                game_id: game_id.to_owned(),
                name: character.name,
                description: character.description,
                status: character.status,
            })
            .await?;
        save_embedded_memory(
            session,
            "character",
            &format!(
                "{} | status: {} | {}",
                character.name, character.status, character.description
            ),
            store,
            llm,
        )
        .await?;
    }

    let existing_facts = store.list_world_facts(game_id, 100).await?;
    for fact in extraction.world_facts {
        if fact.subject.trim().is_empty()
            || fact.predicate.trim().is_empty()
            || fact.object.trim().is_empty()
        {
            continue;
        }

        let existing_id = existing_facts
            .iter()
            .find(|existing| same_fact(existing, &fact.subject, &fact.predicate, &fact.object))
            .map(|existing| existing.id.clone());
        let fact = store
            .upsert_world_fact(UpsertWorldFact {
                id: existing_id,
                game_id: game_id.to_owned(),
                subject: fact.subject,
                predicate: fact.predicate,
                object: fact.object,
                content: fact.content,
                confidence: fact.confidence,
            })
            .await?;
        save_embedded_memory(session, "world_fact", &fact.content, store, llm).await?;
    }

    let existing_locations = store.list_locations(game_id).await?;
    for location in extraction.locations {
        if location.name.trim().is_empty() {
            continue;
        }

        let existing_id = existing_locations
            .iter()
            .find(|existing| same_name(&existing.name, &location.name))
            .map(|existing| existing.id.clone());
        let location = store
            .upsert_location(UpsertLocation {
                id: existing_id,
                game_id: game_id.to_owned(),
                name: location.name,
                description: location.description,
            })
            .await?;
        save_embedded_memory(
            session,
            "location",
            &format!("{} | {}", location.name, location.description),
            store,
            llm,
        )
        .await?;
    }

    Ok(())
}

async fn save_embedded_memory(
    session: &Session,
    kind: &str,
    content: &str,
    store: &dyn HarpeStore,
    llm: &dyn LlmClient,
) -> Result<()> {
    let content = content.trim();
    if content.is_empty() {
        return Ok(());
    }

    let embedding = llm.embed(content).await?;
    store
        .save_memory_chunk(NewMemoryChunk {
            session_id: session.id.clone(),
            kind: kind.to_owned(),
            content: content.to_owned(),
            embedding,
        })
        .await?;

    Ok(())
}

fn same_name(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn same_fact(fact: &WorldFact, subject: &str, predicate: &str, object: &str) -> bool {
    same_name(&fact.subject, subject)
        && same_name(&fact.predicate, predicate)
        && same_name(&fact.object, object)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn update_memory_payload_round_trips_json_value() {
        let payload = UpdateMemoryAfterTurnPayload::new(
            "game-1".to_owned(),
            "session-1".to_owned(),
            "message-1".to_owned(),
            "The gate opens.".to_owned(),
        );

        let decoded =
            UpdateMemoryAfterTurnPayload::from_value(payload.clone().into_value().unwrap())
                .unwrap();

        assert_eq!(decoded, payload);
    }

    #[test]
    fn update_memory_payload_rejects_malformed_json() {
        let err = UpdateMemoryAfterTurnPayload::from_value(serde_json::json!({
            "game_id": "game-1"
        }))
        .unwrap_err();

        assert!(matches!(err, HarpeError::Validation(_)));
    }

    #[test]
    fn extracted_entity_matching_ignores_case_and_whitespace() {
        assert!(same_name(" Mira ", "mira"));
        assert!(!same_name("Mira", "Kest"));

        let fact = WorldFact {
            id: "fact-1".to_owned(),
            game_id: "game-1".to_owned(),
            subject: "Silver Key".to_owned(),
            predicate: "Opens".to_owned(),
            object: "Lower Vault".to_owned(),
            content: "The silver key opens the lower vault.".to_owned(),
            confidence: 1.0,
            updated_at: chrono::Utc::now(),
        };

        assert!(same_fact(&fact, " silver key ", "opens", "lower vault"));
        assert!(!same_fact(&fact, "bronze key", "opens", "lower vault"));
    }

    #[test]
    fn retry_policy_uses_attempts_and_caps_delay() {
        let job = BackgroundJob {
            id: "job-1".to_owned(),
            kind: JobKind::UpdateMemoryAfterTurn,
            status: JobStatus::Running,
            payload: serde_json::json!({}),
            attempts: 2,
            max_attempts: 3,
            last_error: None,
            run_after: Utc::now(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        assert!(should_retry(&job));
        assert_eq!(retry_delay_for_attempt(0), Duration::from_secs(1));
        assert_eq!(retry_delay_for_attempt(2), Duration::from_secs(4));
        assert_eq!(retry_delay_for_attempt(99), Duration::from_secs(256));

        let exhausted = BackgroundJob { attempts: 3, ..job };
        assert!(!should_retry(&exhausted));
    }
}
