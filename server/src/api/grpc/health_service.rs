use super::*;

#[tonic::async_trait]
impl health_service_server::HealthService for HarpeGrpc {
    async fn check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> std::result::Result<Response<pb::HealthCheckResponse>, Status> {
        self.metrics.record_grpc_request();
        let _latency = self.metrics.track_grpc_latency();
        self.metrics.record_health_check();
        tracing::debug!(rpc = "HealthService.Check");
        let service = normalize_health_service(&request.into_inner().service);
        let response = health_response(self.store.as_ref(), service).await;

        Ok(Response::new(response))
    }
}
