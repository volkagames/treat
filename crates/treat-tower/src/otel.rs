//! OpenTelemetry parent-context propagation from incoming request headers.
//! Ported from `treat-actix`'s `otel` module, using `http::HeaderMap`.

use opentelemetry::propagation::Extractor;

struct RequestHeaderCarrier<'a> {
    headers: &'a http::HeaderMap,
}

impl Extractor for RequestHeaderCarrier<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.headers.keys().map(|header| header.as_str()).collect()
    }
}

/// Extract the upstream trace context from `headers`, set it as the span's
/// parent, and record `trace_id` on the span.
pub(crate) fn set_otel_parent(headers: &http::HeaderMap, span: &tracing::Span) {
    use opentelemetry::trace::TraceContextExt as _;
    use tracing_opentelemetry::OpenTelemetrySpanExt as _;

    let parent_context = opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&RequestHeaderCarrier { headers })
    });
    let parented = span.set_parent(parent_context);
    if let Err(err) = parented {
        tracing::debug!(?err, "failed to set OpenTelemetry parent context");
    }
    // Without an OpenTelemetry layer on the subscriber the span has no
    // context and the id is all zeros; leave `trace_id` empty then rather
    // than log a value that matches every other untraced request.
    let trace_id = span.context().span().span_context().trace_id();
    if trace_id != opentelemetry::trace::TraceId::INVALID {
        span.record("trace_id", tracing::field::display(format!("{trace_id:032x}")));
    }
}
