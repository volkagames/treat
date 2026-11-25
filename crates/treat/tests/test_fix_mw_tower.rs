//! Regressions for the tower `TraceLayer` root span on axum: the
//! `otel.status_code` rule shared with actix, `exception.*` from the `treat`
//! error a response carries, and the `http.client_ip` / `http.scheme` fields.
#![cfg(all(feature = "tower-middleware", feature = "axum"))]

use axum::Router;
use axum::body::Body;
use axum::extract::Path;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use tracing::field::{Field, Visit};
use tracing::subscriber::Subscriber;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use treat::tower::TraceLayer;

/// `(field, value)` for every field set on a span named `HTTP request`.
#[derive(Clone, Default)]
struct RootFields(Arc<Mutex<Vec<(String, String)>>>);

impl RootFields {
    fn get(&self, name: &str) -> Option<String> {
        let fields = self.0.lock().expect("capture poisoned");
        fields
            .iter()
            .rev()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.clone())
    }

    fn clear(&self) {
        self.0.lock().expect("capture poisoned").clear();
    }
}

impl Visit for RootFields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .lock()
            .expect("capture poisoned")
            .push((field.name().to_owned(), format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0
            .lock()
            .expect("capture poisoned")
            .push((field.name().to_owned(), value.to_owned()));
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> tracing_subscriber::Layer<S> for RootFields {
    fn on_new_span(&self, attrs: &tracing::span::Attributes<'_>, _id: &tracing::Id, _ctx: Context<'_, S>) {
        if attrs.metadata().name() == "HTTP request" {
            attrs.record(&mut self.clone());
        }
    }

    fn on_record(&self, id: &tracing::Id, values: &tracing::span::Record<'_>, ctx: Context<'_, S>) {
        let is_root = ctx.span(id).is_some_and(|span| span.name() == "HTTP request");
        if is_root {
            values.record(&mut self.clone());
        }
    }
}

async fn api(Path(status): Path<u16>) -> Result<String, treat::ApiError> {
    Err(treat::error("boom").with_message("m").with_status(status))
}

async fn plain(Path(status): Path<u16>) -> impl IntoResponse {
    StatusCode::from_u16(status).expect("status")
}

fn app() -> Router {
    Router::new()
        .route("/api/{status}", get(api))
        .route("/plain/{status}", get(plain))
        .layer(TraceLayer::new())
}

/// `otel.status_code` is `ERROR` iff the status is a 5xx (the actix rule), and
/// the `treat` error in the response extensions fills `exception.*`, which the
/// layer used to declare and never record.
#[tokio::test]
async fn status_and_exception_fields_match_actix() {
    let fields = RootFields::default();
    let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(fields.clone()));

    for (uri, status, otel, exception) in [
        ("/plain/200", 200, "OK", false),
        ("/plain/404", 404, "OK", false),
        ("/plain/503", 503, "ERROR", false),
        ("/api/200", 200, "OK", true),
        ("/api/404", 404, "OK", true),
        ("/api/500", 500, "ERROR", true),
    ] {
        fields.clear();
        let response = app()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).expect("request"))
            .await
            .expect("response");
        assert_eq!(response.status().as_u16(), status, "{uri}");
        assert_eq!(fields.get("otel.status_code").as_deref(), Some(otel), "{uri}");
        assert_eq!(fields.get("http.status_code"), Some(status.to_string()), "{uri}");
        let message = fields.get("exception.message");
        let details = fields.get("exception.details");
        if exception {
            assert_eq!(message.as_deref(), Some("treat error: boom, message: m"), "{uri}");
            assert!(details.is_some_and(|d| d.contains("boom")), "{uri}");
        } else {
            assert_eq!(message, None, "{uri}");
            assert_eq!(details, None, "{uri}");
        }
    }
}

/// `http.client_ip` is recorded like actix's `realip_remote_addr`, and
/// `http.scheme` falls back to `http` rather than an empty string.
#[tokio::test]
async fn client_ip_and_scheme_are_recorded() {
    let fields = RootFields::default();
    let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(fields.clone()));

    let request = Request::builder()
        .uri("/plain/200")
        .header("x-forwarded-for", "203.0.113.9, 10.0.0.1")
        .body(Body::empty())
        .expect("request");
    app().oneshot(request).await.expect("response");

    assert_eq!(fields.get("http.client_ip").as_deref(), Some("203.0.113.9"));
    assert_eq!(fields.get("http.scheme").as_deref(), Some("http"));
}

/// With `tower-telemetry` but no OpenTelemetry layer on the subscriber,
/// `trace_id` stays empty instead of 32 zeros.
#[cfg(feature = "tower-telemetry")]
#[tokio::test]
async fn no_otel_layer_leaves_trace_id_empty() {
    let fields = RootFields::default();
    let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(fields.clone()));
    app()
        .oneshot(
            Request::builder()
                .uri("/plain/200")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert!(fields.get("request_id").is_some());
    assert_eq!(fields.get("trace_id"), None);
}
