use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

const GRPC_LATENCY_BUCKET_MS: [u64; 11] =
    [5, 10, 25, 50, 100, 250, 500, 1_000, 2_500, 5_000, 10_000];

#[derive(Debug)]
pub struct AppMetrics {
    grpc_requests: AtomicU64,
    grpc_failures: AtomicU64,
    streamed_messages: AtomicU64,
    jobs_processed: AtomicU64,
    jobs_succeeded: AtomicU64,
    jobs_retried: AtomicU64,
    jobs_failed: AtomicU64,
    health_checks: AtomicU64,
    grpc_latency_count: AtomicU64,
    grpc_latency_sum_ms: AtomicU64,
    grpc_latency_buckets: [AtomicU64; GRPC_LATENCY_BUCKET_MS.len() + 1],
}

impl Default for AppMetrics {
    fn default() -> Self {
        Self {
            grpc_requests: AtomicU64::new(0),
            grpc_failures: AtomicU64::new(0),
            streamed_messages: AtomicU64::new(0),
            jobs_processed: AtomicU64::new(0),
            jobs_succeeded: AtomicU64::new(0),
            jobs_retried: AtomicU64::new(0),
            jobs_failed: AtomicU64::new(0),
            health_checks: AtomicU64::new(0),
            grpc_latency_count: AtomicU64::new(0),
            grpc_latency_sum_ms: AtomicU64::new(0),
            grpc_latency_buckets: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

pub type SharedMetrics = Arc<AppMetrics>;

impl AppMetrics {
    pub fn shared() -> SharedMetrics {
        Arc::new(Self::default())
    }

    pub fn record_grpc_request(&self) {
        self.grpc_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn track_grpc_latency(&self) -> LatencyGuard<'_> {
        LatencyGuard {
            metrics: self,
            start: Instant::now(),
        }
    }

    pub fn record_grpc_latency(&self, elapsed: Duration) {
        let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        self.grpc_latency_count.fetch_add(1, Ordering::Relaxed);
        self.grpc_latency_sum_ms
            .fetch_add(elapsed_ms, Ordering::Relaxed);
        let bucket_index = GRPC_LATENCY_BUCKET_MS
            .iter()
            .position(|boundary| elapsed_ms <= *boundary)
            .unwrap_or(GRPC_LATENCY_BUCKET_MS.len());
        self.grpc_latency_buckets[bucket_index].fetch_add(1, Ordering::Relaxed);
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
            grpc_latency_count: self.grpc_latency_count.load(Ordering::Relaxed),
            grpc_latency_sum_ms: self.grpc_latency_sum_ms.load(Ordering::Relaxed),
            grpc_latency_buckets: self.grpc_latency_buckets_snapshot(),
            collected_at: Utc::now(),
        }
    }

    pub fn export_prometheus(&self) -> String {
        self.snapshot().to_prometheus()
    }

    fn grpc_latency_buckets_snapshot(&self) -> Vec<LatencyBucketSnapshot> {
        let mut cumulative = 0_u64;
        GRPC_LATENCY_BUCKET_MS
            .into_iter()
            .map(Some)
            .chain(std::iter::once(None))
            .enumerate()
            .map(|(index, upper_bound_ms)| {
                cumulative = cumulative
                    .saturating_add(self.grpc_latency_buckets[index].load(Ordering::Relaxed));
                LatencyBucketSnapshot {
                    upper_bound_ms,
                    count: cumulative,
                }
            })
            .collect()
    }
}

pub struct LatencyGuard<'a> {
    metrics: &'a AppMetrics,
    start: Instant,
}

impl Drop for LatencyGuard<'_> {
    fn drop(&mut self) {
        self.metrics.record_grpc_latency(self.start.elapsed());
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
    pub grpc_latency_count: u64,
    pub grpc_latency_sum_ms: u64,
    pub grpc_latency_buckets: Vec<LatencyBucketSnapshot>,
    pub collected_at: DateTime<Utc>,
}

impl MetricsSnapshot {
    pub fn to_prometheus(&self) -> String {
        let mut lines = Vec::new();
        push_counter(&mut lines, "harpe_grpc_requests_total", self.grpc_requests);
        push_counter(&mut lines, "harpe_grpc_failures_total", self.grpc_failures);
        push_counter(
            &mut lines,
            "harpe_streamed_messages_total",
            self.streamed_messages,
        );
        push_counter(
            &mut lines,
            "harpe_jobs_processed_total",
            self.jobs_processed,
        );
        push_counter(
            &mut lines,
            "harpe_jobs_succeeded_total",
            self.jobs_succeeded,
        );
        push_counter(&mut lines, "harpe_jobs_retried_total", self.jobs_retried);
        push_counter(&mut lines, "harpe_jobs_failed_total", self.jobs_failed);
        push_counter(&mut lines, "harpe_health_checks_total", self.health_checks);
        lines.push("# TYPE harpe_grpc_request_duration_milliseconds histogram".to_owned());
        for bucket in &self.grpc_latency_buckets {
            lines.push(format!(
                "harpe_grpc_request_duration_milliseconds_bucket{{le=\"{}\"}} {}",
                bucket.le_label(),
                bucket.count
            ));
        }
        lines.push(format!(
            "harpe_grpc_request_duration_milliseconds_sum {}",
            self.grpc_latency_sum_ms
        ));
        lines.push(format!(
            "harpe_grpc_request_duration_milliseconds_count {}",
            self.grpc_latency_count
        ));
        lines.push(format!(
            "harpe_metrics_collected_at_seconds {}",
            self.collected_at.timestamp()
        ));
        lines.push(String::new());

        lines.join("\n")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LatencyBucketSnapshot {
    pub upper_bound_ms: Option<u64>,
    pub count: u64,
}

impl LatencyBucketSnapshot {
    pub fn le_label(&self) -> String {
        self.upper_bound_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "+Inf".to_owned())
    }
}

fn push_counter(lines: &mut Vec<String>, name: &str, value: u64) {
    lines.push(format!("# TYPE {name} counter"));
    lines.push(format!("{name} {value}"));
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
        metrics.record_grpc_latency(Duration::from_millis(17));
        metrics.record_grpc_latency(Duration::from_millis(1_500));

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.grpc_requests, 1);
        assert_eq!(snapshot.grpc_failures, 1);
        assert_eq!(snapshot.streamed_messages, 1);
        assert_eq!(snapshot.jobs_processed, 1);
        assert_eq!(snapshot.jobs_succeeded, 1);
        assert_eq!(snapshot.jobs_retried, 1);
        assert_eq!(snapshot.jobs_failed, 1);
        assert_eq!(snapshot.health_checks, 1);
        assert_eq!(snapshot.grpc_latency_count, 2);
        assert_eq!(snapshot.grpc_latency_sum_ms, 1_517);
        assert_eq!(snapshot.grpc_latency_buckets.last().unwrap().count, 2);

        let prometheus = snapshot.to_prometheus();
        assert!(prometheus.contains("harpe_grpc_requests_total 1"));
        assert!(prometheus.contains("harpe_grpc_request_duration_milliseconds_bucket"));
        assert!(prometheus.contains("le=\"+Inf\""));
    }
}
