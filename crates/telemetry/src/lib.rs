//! Optional OpenTelemetry tracing bridge for mimofan (#726, slice A).
//!
//! This crate is **feature-gated and inert by default**: building without the
//! `otlp` feature compiles zero OpenTelemetry dependencies and
//! [`init_otel`] returns a [`OtelHandle::Disabled`] handle. This keeps the
//! default `mimofan` build lean and avoids pulling `tonic`/`prost` into every
//! consumer. Enable the `otlp` feature to actually stand up an OTLP exporter
//! and bridge it into the `tracing` ecosystem.
//!
//! Slice A only provides the initialization seam + the `tracing` bridge hook.
//! Later slices wire specific spans (tool calls, LLM turns, memory writes).

use anyhow::Result;

/// Handle returned by [`init_otel`].
pub enum OtelHandle {
    /// A live OTLP-backed tracer provider is installed and bridged to
    /// `tracing`. Dropping it flushes/shuts down the provider. Only present
    /// when the `otlp` feature is enabled.
    #[cfg(feature = "otlp")]
    Active(TracerProvider),
    /// Telemetry is compiled out (default, no `otlp` feature). No spans are
    /// exported; callers should treat this as a no-op success.
    Disabled,
}

/// The tracer provider type, only meaningful under the `otlp` feature.
/// We re-export it so callers can hold/flush it without depending on
/// `opentelemetry_sdk` directly.
#[cfg(feature = "otlp")]
pub use opentelemetry_sdk::trace::TracerProvider;

/// Initialize OpenTelemetry export and bridge it into `tracing`.
///
/// * With the `otlp` feature: stands up a `TracerProvider` exporting to
///   `endpoint` over OTLP and installs the `tracing` <-> OpenTelemetry bridge
///   so existing `tracing::info!`/etc. spans flow to the collector.
/// * Without it (default): returns [`OtelHandle::Disabled`] immediately — no
///   network, no dependencies.
///
/// `service_name` labels the telemetry stream.
#[cfg(not(feature = "otlp"))]
pub fn init_otel(_endpoint: &str, _service_name: &str) -> Result<OtelHandle> {
    // The endpoint is part of the API so callers don't branch on the feature;
    // it is simply ignored when telemetry is compiled out.
    Ok(OtelHandle::Disabled)
}

#[cfg(feature = "otlp")]
pub fn init_otel(endpoint: &str, service_name: &str) -> Result<OtelHandle> {
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::Resource;
    use opentelemetry_sdk::trace::TracerProvider;
    use opentelemetry::{KeyValue, trace::TracerProvider as _};
    use tracing::subscriber::set_global_default;
    use tracing_opentelemetry::OpenTelemetryLayer;
    use tracing_subscriber::layer::SubscriberExt;

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build OTLP span exporter: {e}"))?;

    let provider = TracerProvider::builder()
        .with_simple_exporter(exporter)
        .with_resource(Resource::new([KeyValue::new("service.name", service_name.to_string())]))
        .build();

    // Bridge OpenTelemetry into the global `tracing` subscriber so existing
    // `tracing` instrumentation is exported. `set_global_default` (not
    // `init`) tolerates an already-installed subscriber (e.g. in tests or
    // when the host app sets its own), degrading to "no export" instead of
    // panicking.
    let tracer = provider.tracer(service_name.to_string());
    let otel_layer = OpenTelemetryLayer::new(tracer);
    let subscriber = tracing_subscriber::registry().with(otel_layer);
    let _ = set_global_default(subscriber);

    Ok(OtelHandle::Active(provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "otlp"))]
    #[tokio::test]
    async fn default_init_is_disabled_and_cheap() {
        // Without the `otlp` feature this must not touch the network and must
        // return the inert handle. Under the `otlp` feature the same call
        // returns an Active handle (covered by `otlp_init_returns_active_handle`).
        let h = init_otel("http://localhost:4317", "mimofan").unwrap();
        assert!(matches!(h, OtelHandle::Disabled));
    }

    #[test]
    fn handle_discriminants_are_distinct() {
        // Compile-time sanity that the enum models both states; the Active
        // arm is only constructible under the `otlp` feature (verified there).
        let disabled = OtelHandle::Disabled;
        assert!(matches!(disabled, OtelHandle::Disabled));
    }

    #[cfg(feature = "otlp")]
    #[tokio::test]
    async fn otlp_init_returns_active_handle() {
        // Only compiled when the feature is on; we point at a non-existent
        // endpoint but the provider builds (export is lazy on span flush).
        let h = init_otel("http://127.0.0.1:4317", "mimofan-test").unwrap();
        assert!(matches!(h, OtelHandle::Active(_)));
    }
}
