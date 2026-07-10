use std::io::Write;

use harpe_proto::pb::{
    ExportMetricsRequest, GetMetricsRequest, MetricsExportFormat,
    metrics_service_client::MetricsServiceClient,
};
use tonic::transport::Channel;

use crate::output::{metrics_json, write_json, write_path_result};
use crate::{CliResult, MetricsArgs, MetricsCommand};

pub(crate) async fn metrics<W: Write>(
    channel: Channel,
    args: MetricsArgs,
    as_json: bool,
    writer: &mut W,
) -> CliResult<()> {
    match args.command {
        None => {
            let response = MetricsServiceClient::new(channel)
                .get_metrics(GetMetricsRequest {})
                .await?
                .into_inner();
            if as_json {
                write_json(writer, &metrics_json(&response))
            } else {
                writeln!(
                    writer,
                    "grpc_requests={} grpc_failures={} streamed_messages={} jobs_processed={} jobs_succeeded={} jobs_retried={} jobs_failed={} health_checks={} collected_at={}",
                    response.grpc_requests,
                    response.grpc_failures,
                    response.streamed_messages,
                    response.jobs_processed,
                    response.jobs_succeeded,
                    response.jobs_retried,
                    response.jobs_failed,
                    response.health_checks,
                    response.collected_at
                )?;
                Ok(())
            }
        }
        Some(MetricsCommand::Export { out }) => {
            let response = MetricsServiceClient::new(channel)
                .export_metrics(ExportMetricsRequest {
                    format: MetricsExportFormat::PrometheusText as i32,
                })
                .await?
                .into_inner();
            if let Some(path) = out {
                std::fs::write(&path, response.body)?;
                write_path_result(writer, as_json, "metrics_path", &path)
            } else {
                write!(writer, "{}", response.body)?;
                Ok(())
            }
        }
    }
}
