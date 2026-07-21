//! OTLP trace export initialisation.
//!
//! `init_subscriber()` replaces the plain `tracing_subscriber::fmt().init()` in
//! `main.rs`.  When the `OTLP_ENDPOINT` environment variable is set it composes
//! a `tracing-opentelemetry` layer into the subscriber so that every
//! `tracing::span!` / `tracing::info!` is automatically exported as an OTLP
//! span to Jaeger / Tempo / any OTel Collector.

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Initialise the tracing subscriber, optionally with OTLP export.
///
/// Returns `Some(provider)` when OTLP was configured; keep it alive and call
/// `.shutdown()` before exit.
pub fn init_subscriber(
    default_filter: &str,
    service_name: &str,
) -> Option<opentelemetry_sdk::trace::SdkTracerProvider> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_filter));

    let endpoint = match std::env::var("OTLP_ENDPOINT") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer())
                .init();
            return None;
        }
    };

    let exporter = match opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .with_timeout(std::time::Duration::from_secs(3))
        .build()
    {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to build OTLP exporter: {}", e);
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer())
                .init();
            return None;
        }
    };

    let resource = Resource::builder()
        .with_service_name(service_name.to_string())
        .build();

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    let tracer = provider.tracer("aigateway");

    // Use the free function `layer()` (not `OpenTelemetryLayer::new()`) so
    // the subscriber type parameter `S` stays generic and can be inferred
    // from the composition context — `with_tracer` preserves the generic `S`.
    let otlp_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .with(otlp_layer)
        .try_init()
        .ok();

    tracing::info!(
        endpoint = %endpoint,
        "OTLP trace export enabled",
    );

    Some(provider)
}
