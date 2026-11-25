//! With `telemetry` and an OpenTelemetry layer on the subscriber, the
//! `TraceLayer` root span takes its parent from the incoming `traceparent` and
//! records that trace's id as `trace_id`.
//!
//! No OpenTelemetry SDK: `tracing_opentelemetry::layer()` runs on the no-op
//! tracer, whose spans carry the parent's span context, and a minimal
//! `traceparent` propagator stands in for the SDK's `TraceContextPropagator`.
#![cfg(feature = "telemetry")]

use opentelemetry::propagation::text_map_propagator::FieldIter;
use opentelemetry::propagation::{Extractor, Injector, TextMapPropagator};
use opentelemetry::trace::{SpanContext, SpanId, TraceContextExt as _, TraceFlags, TraceId, TraceState};
use std::sync::{Arc, Mutex};
use tower::{Layer as _, ServiceExt as _};
use tracing::field::{Field, Visit};
use tracing::subscriber::Subscriber;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use treat_tower::TraceLayer;

const TRACE_ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

/// Reads a W3C `traceparent` header into a remote parent context.
#[derive(Debug)]
struct Traceparent(Vec<String>);

impl TextMapPropagator for Traceparent {
    fn inject_context(&self, _cx: &opentelemetry::Context, _injector: &mut dyn Injector) {}

    fn extract_with_context(&self, cx: &opentelemetry::Context, extractor: &dyn Extractor) -> opentelemetry::Context {
        let Some(header) = extractor.get("traceparent") else {
            return cx.clone();
        };
        let parts: Vec<&str> = header.split('-').collect();
        let [_, trace_id, span_id, _] = parts[..] else {
            return cx.clone();
        };
        let trace_id = TraceId::from_hex(trace_id).expect("trace id");
        let span_id = SpanId::from_hex(span_id).expect("span id");
        let parent = SpanContext::new(trace_id, span_id, TraceFlags::SAMPLED, true, TraceState::default());
        cx.with_remote_span_context(parent)
    }

    fn fields(&self) -> FieldIter<'_> {
        FieldIter::new(&self.0)
    }
}

/// `trace_id` values recorded on spans named `HTTP request`.
#[derive(Clone, Default)]
struct TraceIds(Arc<Mutex<Vec<String>>>);

impl Visit for TraceIds {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "trace_id" {
            self.0.lock().expect("capture poisoned").push(format!("{value:?}"));
        }
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> tracing_subscriber::Layer<S> for TraceIds {
    fn on_record(&self, id: &tracing::Id, values: &tracing::span::Record<'_>, ctx: Context<'_, S>) {
        let is_root = ctx.span(id).is_some_and(|span| span.name() == "HTTP request");
        if is_root {
            values.record(&mut self.clone());
        }
    }
}

/// One request through the `TraceLayer`, with `traceparent` when given; the
/// `trace_id`s the root span recorded.
async fn trace_ids_for(traceparent: Option<&str>) -> Vec<String> {
    opentelemetry::global::set_text_map_propagator(Traceparent(vec!["traceparent".to_owned()]));
    let trace_ids = TraceIds::default();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer())
        .with(trace_ids.clone());
    let _guard = tracing::subscriber::set_default(subscriber);

    let service = TraceLayer::new().layer(tower::service_fn(|_req: http::Request<()>| {
        async { Ok::<_, std::convert::Infallible>(http::Response::new(())) }
    }));
    let mut request = http::Request::builder().uri("/");
    if let Some(traceparent) = traceparent {
        request = request.header("traceparent", traceparent);
    }
    let request = request.body(()).expect("request");
    let response = service.oneshot(request).await.expect("infallible");
    assert!(response.status().is_success());

    trace_ids.0.lock().expect("capture poisoned").clone()
}

#[tokio::test]
async fn records_the_incoming_trace_id() {
    assert_eq!(trace_ids_for(Some(TRACEPARENT)).await, vec![TRACE_ID.to_owned()]);
}

/// Without `traceparent` the no-op tracer has no trace to join, so there is no
/// valid id and `trace_id` stays empty rather than 32 zeros.
#[tokio::test]
async fn no_incoming_trace_leaves_trace_id_empty() {
    assert!(trace_ids_for(None).await.is_empty());
}
