//! Console logging plus optional OTLP/gRPC export of logs, metrics and traces.

use std::env;

use opentelemetry::propagation::TextMapPropagator as _;
use opentelemetry::{global, propagation::Extractor, trace::TracerProvider as _};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource, logs::SdkLoggerProvider, metrics::SdkMeterProvider,
    propagation::TraceContextPropagator, trace::SdkTracerProvider,
};
use thiserror::Error;
use tracing::{Level, Subscriber};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::{EnvFilter, Layer, filter::Targets, fmt, prelude::*};

/// Call once inside the Tokio runtime, before constructing application metrics.
pub fn init() -> Result<Option<Telemetry>, Error> {
    let telemetry = match env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Ok(endpoint) if endpoint.trim().is_empty() => {
            return Err(Error::Config(
                "OTEL_EXPORTER_OTLP_ENDPOINT must not be empty",
            ));
        }
        Ok(endpoint) => Some(Telemetry::new(&endpoint)?),
        Err(env::VarError::NotPresent) => None,
        Err(_) => return Err(Error::Config("OTEL_EXPORTER_OTLP_ENDPOINT must be Unicode")),
    };
    let format = env::var("TELEMETRY_FORMAT").unwrap_or_else(|_| "pretty".to_owned());
    if !matches!(format.as_str(), "pretty" | "json") {
        return Err(Error::Config("TELEMETRY_FORMAT must be pretty or json"));
    }
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("harpe_server=info,opentelemetry=warn"));
    subscriber(filter, format == "json", telemetry.as_ref())
        .try_init()
        .map_err(|error| Error::Subscriber(error.to_string()))?;
    if let Some(telemetry) = &telemetry {
        global::set_meter_provider(telemetry.metrics.clone());
    }
    Ok(telemetry)
}

pub struct Telemetry {
    traces: SdkTracerProvider,
    logs: SdkLoggerProvider,
    metrics: SdkMeterProvider,
}

impl Telemetry {
    fn new(endpoint: &str) -> Result<Self, Error> {
        let resource = Resource::builder()
            .with_service_name(env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "harpe".to_owned()))
            .build();
        let traces = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()?;
        let logs = opentelemetry_otlp::LogExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()?;
        let metrics = opentelemetry_otlp::MetricExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()?;
        Ok(Self {
            traces: SdkTracerProvider::builder()
                .with_resource(resource.clone())
                .with_batch_exporter(traces)
                .build(),
            logs: SdkLoggerProvider::builder()
                .with_resource(resource.clone())
                .with_batch_exporter(logs)
                .build(),
            metrics: SdkMeterProvider::builder()
                .with_resource(resource)
                .with_periodic_exporter(metrics)
                .build(),
        })
    }

    /// Flush every provider, even if one signal fails, before Tokio shuts down.
    pub async fn shutdown(self) -> Result<(), Error> {
        tokio::task::spawn_blocking(move || {
            let traces = self.traces.shutdown();
            let logs = self.logs.shutdown();
            let metrics = self.metrics.shutdown();
            traces?;
            logs?;
            metrics
        })
        .await??;
        Ok(())
    }
}

fn subscriber(
    filter: EnvFilter,
    json: bool,
    telemetry: Option<&Telemetry>,
) -> impl Subscriber + Send + Sync {
    // Export application events only; re-exporting exporter diagnostics can loop.
    let application = Targets::new().with_target("harpe_server", Level::TRACE);
    let traces = telemetry.map(|t| {
        tracing_opentelemetry::layer()
            .with_tracer(t.traces.tracer("harpe-server"))
            .with_filter(application.clone())
    });
    let logs = telemetry.map(|t| OpenTelemetryTracingBridge::new(&t.logs).with_filter(application));
    let console = if json {
        fmt::layer()
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .boxed()
    } else {
        fmt::layer().boxed()
    };
    tracing_subscriber::registry()
        .with(filter)
        .with(traces)
        .with(logs)
        .with(console)
}

struct Headers<'a>(&'a http::HeaderMap);

impl Extractor for Headers<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key)?.to_str().ok()
    }
    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|key| key.as_str()).collect()
    }
}

/// A server span for every RPC, including health checks, with W3C propagation.
pub fn grpc_span(request: &http::Request<()>) -> tracing::Span {
    let parent = TraceContextPropagator::new().extract(&Headers(request.headers()));
    let span = tracing::info_span!("grpc.request", rpc.system = "grpc",
        rpc.method = %request.uri().path(), otel.kind = "server");
    let _ = span.set_parent(parent);
    span
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid telemetry configuration: {0}")]
    Config(&'static str),
    #[error("could not initialize telemetry: {0}")]
    Exporter(#[from] opentelemetry_otlp::ExporterBuildError),
    #[error("could not install telemetry subscriber: {0}")]
    Subscriber(String),
    #[error("could not flush telemetry: {0}")]
    Shutdown(#[from] opentelemetry_sdk::error::OTelSdkError),
    #[error("telemetry shutdown task failed: {0}")]
    ShutdownTask(#[from] tokio::task::JoinError),
}

#[cfg(test)]
mod otlp_test;

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::TraceContextExt;
    use opentelemetry_sdk::{logs::InMemoryLogExporter, trace::InMemorySpanExporter};

    #[tokio::test]
    async fn rpc_logs_and_traces_keep_the_incoming_trace_context() {
        let spans = InMemorySpanExporter::default();
        let logs = InMemoryLogExporter::default();
        let telemetry = Telemetry {
            traces: SdkTracerProvider::builder()
                .with_simple_exporter(spans.clone())
                .build(),
            logs: SdkLoggerProvider::builder()
                .with_simple_exporter(logs.clone())
                .build(),
            metrics: SdkMeterProvider::builder().build(),
        };
        let request = http::Request::builder()
            .uri("/harpe.v1.HealthService/Check")
            .header(
                "traceparent",
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            )
            .body(())
            .unwrap();
        tracing::subscriber::with_default(
            subscriber(EnvFilter::new("info"), true, Some(&telemetry)),
            || {
                grpc_span(&request).in_scope(|| {
                    assert!(
                        tracing::Span::current()
                            .context()
                            .span()
                            .span_context()
                            .is_valid()
                    );
                    tracing::info!("health check completed");
                });
                tracing::warn!(target: "opentelemetry::test", "do not re-export exporter diagnostics");
            },
        );
        telemetry.traces.force_flush().unwrap();
        telemetry.logs.force_flush().unwrap();
        let spans = spans.get_finished_spans().unwrap();
        let logs = logs.get_emitted_logs().unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(logs.len(), 1);
        assert_eq!(
            spans[0].span_context.trace_id().to_string(),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
        assert_eq!(spans[0].parent_span_id.to_string(), "00f067aa0ba902b7");
        let context = logs[0].record.trace_context().unwrap();
        assert_eq!(context.trace_id, spans[0].span_context.trace_id());
        assert_eq!(context.span_id, spans[0].span_context.span_id());
        telemetry.shutdown().await.unwrap();
    }
}
