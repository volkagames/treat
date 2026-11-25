use actix_web::dev::ServiceRequest;
use opentelemetry::propagation::Extractor;

pub(crate) struct RequestHeaderCarrier<'a> {
    headers: &'a actix_web::http::header::HeaderMap,
}

impl<'a> RequestHeaderCarrier<'a> {
    pub(crate) fn new(headers: &'a actix_web::http::header::HeaderMap) -> Self {
        RequestHeaderCarrier { headers }
    }
}

impl Extractor for RequestHeaderCarrier<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.headers.keys().map(|header| header.as_str()).collect()
    }
}

pub(crate) fn set_otel_parent(req: &ServiceRequest, span: &tracing::Span) {
    use opentelemetry::trace::TraceContextExt as _;
    use tracing_opentelemetry::OpenTelemetrySpanExt as _;

    let parent_context = opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&RequestHeaderCarrier::new(req.headers()))
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
