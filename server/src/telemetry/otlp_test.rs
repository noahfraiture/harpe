//! Verify the real OTLP/gRPC exporters against a loopback receiver.
use super::*;
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry_proto::tonic::collector::{
    logs::v1::{
        ExportLogsServiceRequest, ExportLogsServiceResponse,
        logs_service_server::{LogsService, LogsServiceServer},
    },
    metrics::v1::{
        ExportMetricsServiceRequest, ExportMetricsServiceResponse,
        metrics_service_server::{MetricsService, MetricsServiceServer},
    },
    trace::v1::{
        ExportTraceServiceRequest, ExportTraceServiceResponse,
        trace_service_server::{TraceService, TraceServiceServer},
    },
};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status, transport::Server};

#[derive(Debug)]
enum Signal {
    Logs(ExportLogsServiceRequest),
    Metrics(ExportMetricsServiceRequest),
    Traces(ExportTraceServiceRequest),
}

#[derive(Clone)]
struct Receiver(mpsc::UnboundedSender<Signal>);

#[tonic::async_trait]
impl LogsService for Receiver {
    async fn export(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        self.0.send(Signal::Logs(request.into_inner())).unwrap();
        Ok(Response::new(ExportLogsServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl MetricsService for Receiver {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        self.0.send(Signal::Metrics(request.into_inner())).unwrap();
        Ok(Response::new(ExportMetricsServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl TraceService for Receiver {
    async fn export(
        &self,
        request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        self.0.send(Signal::Traces(request.into_inner())).unwrap();
        Ok(Response::new(ExportTraceServiceResponse::default()))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graceful_shutdown_delivers_all_three_signals_over_otlp_grpc() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (send, mut received) = mpsc::unbounded_channel();
    let receiver = Receiver(send);
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(LogsServiceServer::new(receiver.clone()))
            .add_service(MetricsServiceServer::new(receiver.clone()))
            .add_service(TraceServiceServer::new(receiver))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let telemetry = Telemetry::new(&endpoint).unwrap();
    telemetry
        .metrics
        .meter("harpe-server")
        .u64_counter("harpe.test.requests")
        .build()
        .add(7, &[]);
    tracing::subscriber::with_default(
        subscriber(EnvFilter::new("info"), false, Some(&telemetry)),
        || grpc_span(&http::Request::new(())).in_scope(|| tracing::info!("OTLP round trip")),
    );
    tokio::time::timeout(std::time::Duration::from_secs(10), telemetry.shutdown())
        .await
        .unwrap()
        .unwrap();

    let mut logs = Vec::new();
    let mut spans = Vec::new();
    let mut metrics = Vec::new();
    while let Ok(signal) = received.try_recv() {
        match signal {
            Signal::Logs(request) => logs.extend(
                request
                    .resource_logs
                    .into_iter()
                    .flat_map(|r| r.scope_logs)
                    .flat_map(|s| s.log_records),
            ),
            Signal::Traces(request) => spans.extend(
                request
                    .resource_spans
                    .into_iter()
                    .flat_map(|r| r.scope_spans)
                    .flat_map(|s| s.spans),
            ),
            Signal::Metrics(request) => metrics.extend(
                request
                    .resource_metrics
                    .into_iter()
                    .flat_map(|r| r.scope_metrics)
                    .flat_map(|s| s.metrics),
            ),
        }
    }
    assert_eq!(logs.len(), 1);
    assert_eq!(spans.len(), 1);
    assert_eq!(logs[0].trace_id, spans[0].trace_id);
    assert_eq!(logs[0].span_id, spans[0].span_id);
    assert_eq!(spans[0].name, "grpc.request");
    let metric = metrics
        .iter()
        .find(|m| m.name == "harpe.test.requests")
        .unwrap();
    use opentelemetry_proto::tonic::metrics::v1::{metric::Data, number_data_point::Value};
    let Some(Data::Sum(sum)) = &metric.data else {
        panic!("expected an OTLP counter");
    };
    assert_eq!(sum.data_points[0].value, Some(Value::AsInt(7)));
    stop.send(()).unwrap();
    server.await.unwrap();
}
