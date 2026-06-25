use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;

use crate::domain::{BackgroundJob, JobKind, JobStatus};
use crate::llm::LlmClient;
use crate::observability::{AppMetrics, SharedMetrics};
use crate::store::HarpeStore;
use crate::{HarpeError, Result};

use super::memory_update::update_memory_after_turn;
use super::payload::UpdateMemoryAfterTurnPayload;
use super::retry::{retry_delay_for_attempt, should_retry};

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

    #[tracing::instrument(skip_all)]
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

    #[tracing::instrument(skip_all, fields(job_id = %job.id, job_kind = ?job.kind, attempts = job.attempts))]
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
