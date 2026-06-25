use chrono::Utc;

use super::convert::saturating_u32;
use crate::Result;
use crate::domain::JobStatus;
use crate::pb;
use crate::store::HarpeStore;

pub(super) async fn health_response(
    store: &dyn HarpeStore,
    service: String,
) -> pb::HealthCheckResponse {
    let checked_at = Utc::now().to_rfc3339();
    let health = async {
        store.list_games(1).await?;
        let pending_jobs = store
            .list_jobs(Some(JobStatus::Pending), 1_000)
            .await?
            .len();
        let failed_jobs = store.list_jobs(Some(JobStatus::Failed), 1_000).await?.len();
        Result::Ok((pending_jobs, failed_jobs))
    }
    .await;

    match health {
        Ok((pending_jobs, failed_jobs)) => {
            let status = if failed_jobs > 0 {
                pb::ServingStatus::Degraded
            } else {
                pb::ServingStatus::Serving
            };

            pb::HealthCheckResponse {
                status: status as i32,
                service,
                version: env!("CARGO_PKG_VERSION").to_owned(),
                database_ok: true,
                pending_jobs: saturating_u32(pending_jobs),
                failed_jobs: saturating_u32(failed_jobs),
                checked_at,
            }
        }
        Err(error) => pb::HealthCheckResponse {
            status: pb::ServingStatus::NotServing as i32,
            service,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            database_ok: false,
            pending_jobs: 0,
            failed_jobs: 0,
            checked_at: format!("{checked_at}; error={error}"),
        },
    }
}

pub(super) fn normalize_health_service(service: &str) -> String {
    let service = service.trim();
    if service.is_empty() {
        "harpe.v1".to_owned()
    } else {
        service.to_owned()
    }
}
