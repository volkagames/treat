//! The documented stack — `LoggingLayer` inside, `TraceLayer` outside — records
//! the `RequestId` on this crate's span and echoes it on the response; and
//! `event_dynamic_lvl!` expands without a `tracing` path of the caller's.

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::routing::get;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use tracing::field::{Field, Visit};
use tracing::subscriber::Subscriber;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use treat_axum_logging::LoggingLayer;
use treat_tower::{TraceLayer, X_REQUEST_ID};

/// `(span name, field, value)` for every field set on a span.
#[derive(Clone, Default)]
struct SpanFields(Arc<Mutex<Vec<(String, String, String)>>>);

struct Visitor<'a> {
    span: &'static str,
    out: &'a SpanFields,
}

impl Visit for Visitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.out.0.lock().expect("capture poisoned").push((
            self.span.to_owned(),
            field.name().to_owned(),
            format!("{value:?}"),
        ));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.out.0.lock().expect("capture poisoned").push((
            self.span.to_owned(),
            field.name().to_owned(),
            value.to_owned(),
        ));
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> tracing_subscriber::Layer<S> for SpanFields {
    fn on_new_span(&self, attrs: &tracing::span::Attributes<'_>, _id: &tracing::Id, _ctx: Context<'_, S>) {
        let span = attrs.metadata().name();
        attrs.record(&mut Visitor { span, out: self });
    }

    fn on_record(&self, id: &tracing::Id, values: &tracing::span::Record<'_>, ctx: Context<'_, S>) {
        let span = ctx.span(id).map_or("?", |span| span.name());
        values.record(&mut Visitor { span, out: self });
    }
}

impl SpanFields {
    fn get(&self, span: &str, name: &str) -> Option<String> {
        let fields = self.0.lock().expect("capture poisoned");
        fields
            .iter()
            .rev()
            .find(|(s, field, _)| s == span && field == name)
            .map(|(_, _, value)| value.clone())
    }
}

#[tokio::test]
async fn trace_layer_outside_logging_layer_shares_the_request_id() {
    let fields = SpanFields::default();
    let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(fields.clone()));

    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(LoggingLayer::new())
        .layer(TraceLayer::new());
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).expect("request"))
        .await
        .expect("response");

    let echoed = response
        .headers()
        .get(X_REQUEST_ID)
        .expect("TraceLayer echoes the id")
        .to_str()
        .expect("ascii")
        .to_owned();
    assert_eq!(fields.get("request", "request_id").as_deref(), Some(echoed.as_str()));
    assert_eq!(
        fields.get("HTTP request", "request_id").as_deref(),
        Some(echoed.as_str())
    );
}

/// A local `tracing` module shadows the crate name: an expansion that names a
/// bare `tracing::` path resolves to it and fails to compile.
mod shadowed_tracing {
    #[allow(dead_code)]
    mod tracing {}

    #[test]
    fn event_dynamic_lvl_does_not_need_the_callers_tracing() {
        let level = ::tracing::Level::INFO;
        treat_axum_logging::event_dynamic_lvl!(level, status = 200, "served");
    }
}
