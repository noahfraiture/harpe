use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};

#[derive(Debug, Default)]
pub struct AppMetrics {
    grpc_requests: AtomicU64,
    grpc_failures: AtomicU64,
    streamed_messages: AtomicU64,
    jobs_processed: AtomicU64,
    jobs_succeeded: AtomicU64,
    jobs_retried: AtomicU64,
    jobs_failed: AtomicU64,
    health_checks: AtomicU64,
}

pub type SharedMetrics = Arc<AppMetrics>;

impl AppMetrics {
    pub fn shared() -> SharedMetrics {
        Arc::new(Self::default())
    }

    pub fn record_grpc_request(&self) {
        self.grpc_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_grpc_failure(&self) {
        self.grpc_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_streamed_message(&self) {
        self.streamed_messages.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_job_processed(&self) {
        self.jobs_processed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_job_succeeded(&self) {
        self.jobs_succeeded.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_job_retried(&self) {
        self.jobs_retried.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_job_failed(&self) {
        self.jobs_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_health_check(&self) {
        self.health_checks.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            grpc_requests: self.grpc_requests.load(Ordering::Relaxed),
            grpc_failures: self.grpc_failures.load(Ordering::Relaxed),
            streamed_messages: self.streamed_messages.load(Ordering::Relaxed),
            jobs_processed: self.jobs_processed.load(Ordering::Relaxed),
            jobs_succeeded: self.jobs_succeeded.load(Ordering::Relaxed),
            jobs_retried: self.jobs_retried.load(Ordering::Relaxed),
            jobs_failed: self.jobs_failed.load(Ordering::Relaxed),
            health_checks: self.health_checks.load(Ordering::Relaxed),
            collected_at: Utc::now(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub grpc_requests: u64,
    pub grpc_failures: u64,
    pub streamed_messages: u64,
    pub jobs_processed: u64,
    pub jobs_succeeded: u64,
    pub jobs_retried: u64,
    pub jobs_failed: u64,
    pub health_checks: u64,
    pub collected_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_snapshot_reports_recorded_counts() {
        let metrics = AppMetrics::default();

        metrics.record_grpc_request();
        metrics.record_grpc_failure();
        metrics.record_streamed_message();
        metrics.record_job_processed();
        metrics.record_job_succeeded();
        metrics.record_job_retried();
        metrics.record_job_failed();
        metrics.record_health_check();

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.grpc_requests, 1);
        assert_eq!(snapshot.grpc_failures, 1);
        assert_eq!(snapshot.streamed_messages, 1);
        assert_eq!(snapshot.jobs_processed, 1);
        assert_eq!(snapshot.jobs_succeeded, 1);
        assert_eq!(snapshot.jobs_retried, 1);
        assert_eq!(snapshot.jobs_failed, 1);
        assert_eq!(snapshot.health_checks, 1);
    }
}
