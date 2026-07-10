use tonic::{Request, Response, Status};

use crate::HarpeError;
use crate::pb::{self, ExportMetricsRequest, GetMetricsRequest, metrics_service_server};

use super::HarpeGrpc;
use super::convert::{metrics_to_pb, status_from_error};

#[tonic::async_trait]
impl metrics_service_server::MetricsService for HarpeGrpc {
    async fn get_metrics(
        &self,
        _request: Request<GetMetricsRequest>,
    ) -> std::result::Result<Response<pb::MetricsSnapshot>, Status> {
        let _request = self.observe_request("MetricsService.GetMetrics");

        Ok(Response::new(metrics_to_pb(self.metrics.snapshot())))
    }

    async fn export_metrics(
        &self,
        request: Request<ExportMetricsRequest>,
    ) -> std::result::Result<Response<pb::ExportMetricsResponse>, Status> {
        let _request = self.observe_request("MetricsService.ExportMetrics");
        let request = request.into_inner();
        let format = pb::MetricsExportFormat::try_from(request.format).map_err(|_| {
            status_from_error(HarpeError::Validation(format!(
                "unknown metrics export format {}",
                request.format
            )))
        })?;
        match format {
            pb::MetricsExportFormat::Unspecified | pb::MetricsExportFormat::PrometheusText => {
                Ok(Response::new(pb::ExportMetricsResponse {
                    content_type: "text/plain; version=0.0.4; charset=utf-8".to_owned(),
                    body: self.metrics.export_prometheus(),
                }))
            }
        }
    }
}
