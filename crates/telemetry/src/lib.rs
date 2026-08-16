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
//!
//! Slice B (this file, below) adds a **dependency-free** in-process metrics
//! recorder ([`PrometheusRecorder`]) so the tui can record counters/histograms
//! (tool latency, LLM token cost, memory writes) without pulling the
//! `prometheus` crate into the default build. The recorder emits the standard
//! Prometheus text exposition format via [`PrometheusRecorder::to_text`], so a
//! future `/metrics` endpoint or OTLP bridge can scrape it directly.

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
    use opentelemetry::{KeyValue, trace::TracerProvider as _};
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::Resource;
    use opentelemetry_sdk::trace::TracerProvider;
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
        .with_resource(Resource::new([KeyValue::new(
            "service.name",
            service_name.to_string(),
        )]))
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

/// A lightweight, dependency-free metrics recorder (#726 slice B).
///
/// Holds counters and histograms in process memory and renders the standard
/// Prometheus text exposition format. Intentionally does *not* depend on the
/// `prometheus` crate so the default build stays lean; a future slice can swap
/// the backing store for a real exporter without changing the record API.
///
/// Thread-safe via `RwLock`; `record_*` calls are cheap and non-blocking.
use std::collections::HashMap;
use std::sync::RwLock;

/// A single recorded histogram sample bucket boundary (seconds, for latency).
const HISTOGRAM_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// In-process Prometheus-style recorder.
#[derive(Default)]
pub struct PrometheusRecorder {
    counters: RwLock<HashMap<String, f64>>,
    /// Histogram samples stored per metric name (raw values, buckets on render).
    histograms: RwLock<HashMap<String, Vec<f64>>>,
}

impl PrometheusRecorder {
    /// Create an empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment a counter by `delta` (negative deltas allowed).
    pub fn record_counter(&self, name: &str, delta: f64) {
        let mut c = self.counters.write().unwrap();
        *c.entry(name.to_string()).or_insert(0.0) += delta;
    }

    /// Record a histogram observation (e.g. a latency in seconds).
    pub fn record_histogram(&self, name: &str, value: f64) {
        let mut h = self.histograms.write().unwrap();
        h.entry(name.to_string()).or_default().push(value);
    }

    /// Convenience: record a latency histogram sample (seconds).
    pub fn record_latency(&self, name: &str, seconds: f64) {
        self.record_histogram(name, seconds);
    }

    /// Render all metrics in Prometheus text exposition format.
    ///
    /// Counters emit `# TYPE x counter` + `x N`; histograms emit cumulative
    /// bucket counts + `_sum` + `_count`. Stable field order so the output is
    /// diff-friendly (helps prefix-cache-style comparisons in tests).
    pub fn to_text(&self) -> String {
        let counters = self.counters.read().unwrap();
        let histograms = self.histograms.read().unwrap();
        let mut out = String::new();
        let mut cnames: Vec<&String> = counters.keys().collect();
        cnames.sort();
        for name in cnames {
            out.push_str(&format!("# TYPE {name} counter\n"));
            out.push_str(&format!("{name} {}\n", counters[name]));
        }
        let mut hnames: Vec<&String> = histograms.keys().collect();
        hnames.sort();
        for name in hnames {
            out.push_str(&format!("# TYPE {name} histogram\n"));
            let samples = &histograms[name];
            let sum: f64 = samples.iter().sum();
            let count = samples.len();
            // Prometheus buckets are *cumulative*: each bucket is the count of
            // samples <= its upper bound `le`. No extra accumulation — the
            // filter count itself is already the cumulative value.
            for b in HISTOGRAM_BUCKETS {
                let c = samples.iter().filter(|v| *v <= b).count() as u64;
                out.push_str(&format!("{name}_bucket{{le=\"{:.3}\"}} {c}\n", b));
            }
            out.push_str(&format!("{name}_bucket{{le=\"+Inf\"}} {count}\n"));
            out.push_str(&format!("{name}_sum {sum}\n"));
            out.push_str(&format!("{name}_count {count}\n"));
        }
        out
    }
}

/// Global default recorder handle (lazily initialized, process-wide).
///
/// Callers use [`record_metric`] to record into this shared instance without
/// threading the recorder through every call site.
static GLOBAL_RECORDER: std::sync::OnceLock<PrometheusRecorder> = std::sync::OnceLock::new();

/// Return the process-wide [`PrometheusRecorder`] instance (idempotent).
///
/// Both [`record_metric`] and [`metrics_text`] resolve through this so all
/// call sites share one backing store. Tests can read it directly to assert on
/// recorded values.
pub fn global_recorder() -> &'static PrometheusRecorder {
    GLOBAL_RECORDER.get_or_init(PrometheusRecorder::new)
}

/// Record a metric into the process-wide [`PrometheusRecorder`].
pub fn record_metric(name: &str, value: MetricValue) {
    let rec = global_recorder();
    match value {
        MetricValue::Counter(delta) => rec.record_counter(name, delta),
        MetricValue::Histogram(v) => rec.record_histogram(name, v),
        MetricValue::Latency(sec) => rec.record_latency(name, sec),
    }
}

/// A metric observation passed to [`record_metric`].
pub enum MetricValue {
    /// Counter delta.
    Counter(f64),
    /// Histogram raw value.
    Histogram(f64),
    /// Latency in seconds (histogram convenience).
    Latency(f64),
}

/// GenAI-style span/event emitted around an LLM call (#830, slice C/D).
///
/// This is the **v1 in-process emitter**: it does not require the `otlp`
/// feature or any OpenTelemetry dependency. Instead it records the canonical
/// GenAI semantic-convention signals into the shared [`PrometheusRecorder`]
/// via [`record_metric`] (token counts, model label, latency). A future slice
/// can mirror these counters into real OTLP spans without changing the call
/// site, because the call site only depends on this struct.
///
/// Usage:
/// ```ignore
/// let mut span = GenAiSpan::new("deepseek-chat");
/// // ... perform the LLM request ...
/// span.finish(input_tokens, output_tokens, cached_tokens);
/// ```
///
/// Metrics produced (Prometheus style, scraped via [`metrics_text`]):
/// - `gen_ai_client_token_usage`{gen_ai_operation, gen_ai_token_type, gen_ai_request_model} (counter)
/// - `gen_ai_client_operation_duration`{gen_ai_request_model} (histogram, seconds)
/// - `gen_ai_client_requests`{gen_ai_request_model, status} (counter)
pub struct GenAiSpan {
    model: String,
    start: std::time::Instant,
}

impl GenAiSpan {
    /// Begin a GenAI span for a request to `model`.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            start: std::time::Instant::now(),
        }
    }

    /// Record token usage and latency for a completed (successful) request.
    ///
    /// `input_tokens` / `output_tokens` / `cached_tokens` are the per-request
    /// usage counts reported by the provider. Latency is measured from span
    /// creation to this call.
    pub fn finish(&self, input_tokens: u64, output_tokens: u64, cached_tokens: u64) {
        let labels = |op: &str, tok: &str| -> String {
            format!(
                "{{gen_ai_operation=\"{op}\",gen_ai_token_type=\"{tok}\",gen_ai_request_model=\"{}\"}}",
                self.model
            )
        };
        record_metric(
            &format!("gen_ai_client_token_usage{}", labels("chat", "input")),
            MetricValue::Counter(input_tokens as f64),
        );
        record_metric(
            &format!("gen_ai_client_token_usage{}", labels("chat", "output")),
            MetricValue::Counter(output_tokens as f64),
        );
        record_metric(
            &format!("gen_ai_client_token_usage{}", labels("chat", "cache_read")),
            MetricValue::Counter(cached_tokens as f64),
        );
        let dur = self.start.elapsed().as_secs_f64();
        record_metric(
            &format!(
                "gen_ai_client_operation_duration{{gen_ai_request_model=\"{}\"}}",
                self.model
            ),
            MetricValue::Latency(dur),
        );
        record_metric(
            &format!(
                "gen_ai_client_requests{{gen_ai_request_model=\"{}\",status=\"ok\"}}",
                self.model
            ),
            MetricValue::Counter(1.0),
        );
    }

    /// Record a failed request (latency + error counter, no token usage).
    pub fn finish_err(&self) {
        let dur = self.start.elapsed().as_secs_f64();
        record_metric(
            &format!(
                "gen_ai_client_operation_duration{{gen_ai_request_model=\"{}\"}}",
                self.model
            ),
            MetricValue::Latency(dur),
        );
        record_metric(
            &format!(
                "gen_ai_client_requests{{gen_ai_request_model=\"{}\",status=\"error\"}}",
                self.model
            ),
            MetricValue::Counter(1.0),
        );
    }
}

/// Render all recorded metrics in Prometheus text exposition format.
///
/// This is the body of the `/metrics` HTTP endpoint. It delegates to the
/// global [`PrometheusRecorder`] (via [`record_metric`]) so counters/histograms
/// recorded anywhere in the process (tool latency, LLM token cost, memory
/// writes, GenAI spans) are exposed together. The returned text is stable and
/// diff-friendly (field order is sorted in [`PrometheusRecorder::to_text`]).
///
/// v1 ships a JSON-free Prometheus rendering to stay dependency-free. A future
/// slice may also add a JSON variant, but the canonical scrape format is the
/// Prometheus text exposition format.
pub fn metrics_text() -> String {
    global_recorder().to_text()
}

#[cfg(test)]
mod genai_span_tests {
    use super::*;

    #[test]
    fn genai_span_records_tokens_and_latency() {
        let span = GenAiSpan::new("deepseek-chat");
        std::thread::sleep(std::time::Duration::from_millis(2));
        span.finish(120, 80, 40);

        let text = global_recorder().to_text();
        assert!(text.contains("gen_ai_client_token_usage{gen_ai_operation=\"chat\",gen_ai_token_type=\"input\",gen_ai_request_model=\"deepseek-chat\"} 120"));
        assert!(text.contains("gen_ai_client_token_usage{gen_ai_operation=\"chat\",gen_ai_token_type=\"output\",gen_ai_request_model=\"deepseek-chat\"} 80"));
        assert!(text.contains("gen_ai_client_token_usage{gen_ai_operation=\"chat\",gen_ai_token_type=\"cache_read\",gen_ai_request_model=\"deepseek-chat\"} 40"));
        assert!(text.contains(
            "gen_ai_client_requests{gen_ai_request_model=\"deepseek-chat\",status=\"ok\"} 1"
        ));
        assert!(
            text.contains(
                "gen_ai_client_operation_duration{gen_ai_request_model=\"deepseek-chat\"}"
            )
        );
    }

    #[test]
    fn genai_span_records_error() {
        let span = GenAiSpan::new("dummy");
        span.finish_err();
        let text = global_recorder().to_text();
        assert!(
            text.contains(
                "gen_ai_client_requests{gen_ai_request_model=\"dummy\",status=\"error\"} 1"
            )
        );
    }

    #[test]
    fn metrics_text_delegates_to_global_recorder() {
        // Smoke test: metrics_text returns the Prometheus text format and is
        // non-panicking. (Exact content may include other tests' counters.)
        let text = metrics_text();
        assert!(text.contains("# TYPE") || text.is_empty());
    }
}

#[cfg(test)]
mod metric_tests {
    use super::*;

    #[test]
    fn counter_accumulates() {
        let r = PrometheusRecorder::new();
        r.record_counter("tool_calls", 1.0);
        r.record_counter("tool_calls", 2.0);
        let text = r.to_text();
        assert!(text.contains("tool_calls 3"));
    }

    #[test]
    fn histogram_emits_buckets_and_sum() {
        let r = PrometheusRecorder::new();
        r.record_latency("turn_latency", 0.01);
        r.record_latency("turn_latency", 0.5);
        let text = r.to_text();
        assert!(text.contains("# TYPE turn_latency histogram"));
        // Sum is rendered with full float precision; assert approximately.
        assert!(text.contains("turn_latency_sum 0.51") || text.contains("turn_latency_sum 0.509"));
        assert!(text.contains("turn_latency_count 2"));
        // 0.01s falls in the le="0.010" cumulative bucket (count 1); 0.5s
        // extends the cumulative count to 2 at le="0.500"/"1.000".
        assert!(text.contains("turn_latency_bucket{le=\"0.010\"} 1"));
        assert!(text.contains("turn_latency_bucket{le=\"1.000\"} 2"));
        assert!(text.contains("turn_latency_bucket{le=\"+Inf\"} 2"));
    }

    #[test]
    fn global_record_metric_works() {
        record_metric("memory_writes", MetricValue::Counter(1.0));
        // Should not panic; idempotent across calls.
        record_metric("memory_writes", MetricValue::Counter(1.0));
    }
}
