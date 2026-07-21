//! OTLP trace export initialisation.
//!
//! When the `OTLP_ENDPOINT` environment variable is set this module builds a
//! gRPC span exporter, registers it as the global OpenTelemetry tracer provider,
//! and returns the provider handle.
//!
//! When the variable is absent the function is a no-op — no OTLP traffic is
//! generated and no connection is attempted.
//!
//! ## Usage
//!
//! Call `init_otlp()` early in `main()` after the tracing subscriber is set up.
//! Keep the returned provider alive for the lifetime of the program, and call
//! `.shutdown()` on it before exit.

use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;

/// Initialise the global OTLP tracer provider.
///
/// Returns `Some(provider)` when `OTLP_ENDPOINT` is set and the exporter was
/// successfully created.  The caller must keep the provider alive across the
/// program lifetime and call `.shutdown()` before exit to flush buffered spans.
pub fn init_otlp(
    service_name: &str,
) -> Option<opentelemetry_sdk::trace::SdkTracerProvider> {
    let endpoint = std::env::var("OTLP_ENDPOINT").ok()?;
    if endpoint.is_empty() {
        return None;
    }

    let exporter = match opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .with_timeout(std::time::Duration::from_secs(3))
        .build()
    {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to build OTLP exporter: {}", e);
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

    // Register as the global tracer provider so that any code using
    // opentelemetry::global::tracer() picks it up.
    opentelemetry::global::set_tracer_provider(provider.clone());

    tracing::info!(
        endpoint = %endpoint,
        "OTLP trace export enabled",
    );

    Some(provider)
}
