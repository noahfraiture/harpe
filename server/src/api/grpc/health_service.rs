use tonic::{Request, Response, Status};

use crate::pb::{self, HealthCheckRequest, health_service_server};

use super::HarpeGrpc;
use super::health::{health_response, normalize_health_service};

#[tonic::async_trait]
impl health_service_server::HealthService for HarpeGrpc {
    async fn check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> std::result::Result<Response<pb::HealthCheckResponse>, Status> {
        let _request = self.observe_request("HealthService.Check");
        self.metrics.record_health_check();
        let service = normalize_health_service(&request.into_inner().service);
        let response = health_response(self.store.as_ref(), service).await;

        Ok(Response::new(response))
    }
}
