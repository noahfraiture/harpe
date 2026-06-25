use std::time::Duration;

use crate::domain::BackgroundJob;

pub(super) fn should_retry(job: &BackgroundJob) -> bool {
    job.attempts < job.max_attempts
}

pub(super) fn retry_delay_for_attempt(attempts: i32) -> Duration {
    let exponent = attempts.clamp(0, 8) as u32;
    Duration::from_secs(2_u64.pow(exponent)).min(Duration::from_secs(300))
}
