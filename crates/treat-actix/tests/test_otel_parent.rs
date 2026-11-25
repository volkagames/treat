//! With `telemetry` and an OpenTelemetry layer on the subscriber, the root span
//! takes its parent from the incoming `traceparent` and records that trace's id
//! as `trace_id` — for the default builder and for a custom one calling
//! `root_span!`. (A custom builder in a crate *without* a `telemetry` feature
//! is covered by `treat-hygiene-renamed`'s `test_root_span_otel`.)
//!
//! No OpenTelemetry SDK: `tracing_opentelemetry::layer()` runs on the no-op
//! tracer, whose spans carry the parent's span context, and a minimal
//! `traceparent` propagator stands in for the SDK's `TraceContextPropagator`.
#![cfg(feature = "telemetry")]

use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::{App, HttpResponse, test as atest, web};
use opentelemetry::propagation::text_map_propagator::FieldIter;
use opentelemetry::propagation::{Extractor, Injector, TextMapPropagator};
use opentelemetry::trace::{SpanContext, SpanId, TraceContextExt as _, TraceFlags, TraceId, TraceState};
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::subscriber::Subscriber;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use treat_actix::{DefaultRootSpanBuilder, Logger, RootSpanBuilder};

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

struct Tenanted;

impl RootSpanBuilder for Tenanted {
    fn on_request_start(request: &ServiceRequest) -> tracing::Span {
        treat_actix::root_span!(request, tenant = "acme")
    }

    fn on_request_end<B>(span: tracing::Span, outcome: &Result<ServiceResponse<B>, actix_web::Error>) {
        DefaultRootSpanBuilder::on_request_end(span, outcome)
    }
}

/// One request with `traceparent` through `Logger::<R>`; the `trace_id`s the
/// root span recorded.
async fn trace_ids_through<R: RootSpanBuilder + 'static>() -> Vec<String> {
    opentelemetry::global::set_text_map_propagator(Traceparent(vec!["traceparent".to_owned()]));
    let trace_ids = TraceIds::default();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer())
        .with(trace_ids.clone());
    let _guard = tracing::subscriber::set_default(subscriber);

    let app = atest::init_service(
        App::new()
            .wrap(Logger::<R>::new())
            .route("/", web::get().to(HttpResponse::Ok)),
    )
    .await;
    let request = atest::TestRequest::get()
        .uri("/")
        .insert_header(("traceparent", TRACEPARENT))
        .to_request();
    let response = atest::call_service(&app, request).await;
    assert!(response.status().is_success());

    trace_ids.0.lock().expect("capture poisoned").clone()
}

#[actix_web::test]
async fn default_builder_records_the_incoming_trace_id() {
    assert_eq!(
        trace_ids_through::<DefaultRootSpanBuilder>().await,
        vec![TRACE_ID.to_owned()]
    );
}

#[actix_web::test]
async fn custom_builder_records_the_incoming_trace_id() {
    assert_eq!(trace_ids_through::<Tenanted>().await, vec![TRACE_ID.to_owned()]);
}

/// Without `traceparent` the no-op tracer has no trace to join, so there is no
/// valid id and `trace_id` stays empty rather than 32 zeros.
#[actix_web::test]
async fn no_incoming_trace_leaves_trace_id_empty() {
    opentelemetry::global::set_text_map_propagator(Traceparent(vec!["traceparent".to_owned()]));
    let trace_ids = TraceIds::default();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer())
        .with(trace_ids.clone());
    let _guard = tracing::subscriber::set_default(subscriber);

    let request = atest::TestRequest::get().uri("/").to_srv_request();
    let _span = DefaultRootSpanBuilder::on_request_start(&request);
    assert!(trace_ids.0.lock().expect("capture poisoned").is_empty());
}
