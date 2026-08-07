//! Coverage for the standalone root-span builders. The span's *fields* are what
//! callers depend on, so the assertions go through a subscriber that captures
//! them rather than poking at the `Span` handle.

use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::subscriber::Subscriber;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use treat_axum_logging::{root_span_on_request_start, root_span_on_response_end};

/// Records every field set on a span, at creation and via `record`.
#[derive(Clone, Default)]
struct FieldCapture(Arc<Mutex<Vec<(String, String)>>>);

impl FieldCapture {
    fn get(&self, name: &str) -> Option<String> {
        let fields = self.0.lock().expect("capture poisoned");
        fields
            .iter()
            .rev()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.clone())
    }
}

impl Visit for FieldCapture {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .lock()
            .expect("capture poisoned")
            .push((field.name().to_string(), format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0
            .lock()
            .expect("capture poisoned")
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0
            .lock()
            .expect("capture poisoned")
            .push((field.name().to_string(), value.to_string()));
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> tracing_subscriber::Layer<S> for FieldCapture {
    fn on_new_span(&self, attrs: &tracing::span::Attributes<'_>, _id: &tracing::Id, _ctx: Context<'_, S>) {
        attrs.record(&mut self.clone());
    }

    fn on_record(&self, _id: &tracing::Id, values: &tracing::span::Record<'_>, _ctx: Context<'_, S>) {
        values.record(&mut self.clone());
    }
}

/// Runs `body` with a subscriber capturing span fields, and returns the capture.
fn capture(body: impl FnOnce()) -> FieldCapture {
    let fields = FieldCapture::default();
    let subscriber = tracing_subscriber::registry().with(fields.clone());
    tracing::subscriber::with_default(subscriber, body);
    fields
}

#[test]
fn records_the_method_and_uri() {
    let fields = capture(|| {
        let request = http::Request::builder()
            .method(http::Method::POST)
            .uri("/users/7")
            .body(())
            .expect("request");
        let _span = root_span_on_request_start(&request);
    });

    assert_eq!(fields.get("request_method").as_deref(), Some("POST"));
    assert_eq!(fields.get("request_uri").as_deref(), Some("/users/7"));
}

#[test]
fn records_the_user_agent_and_forwarded_ip() {
    let fields = capture(|| {
        let request = http::Request::builder()
            .uri("/")
            .header(http::header::USER_AGENT, "curl/8")
            .header("x-forwarded-for", "203.0.113.7")
            .body(())
            .expect("request");
        let _span = root_span_on_request_start(&request);
    });

    assert_eq!(fields.get("http_user_agent").as_deref(), Some("curl/8"));
    assert_eq!(fields.get("http_client_ip").as_deref(), Some("203.0.113.7"));
}

/// A header that is not valid UTF-8 must not panic or abort the span.
#[test]
fn falls_back_on_a_non_utf8_header() {
    let fields = capture(|| {
        let mut request = http::Request::builder().uri("/").body(()).expect("request");
        request.headers_mut().insert(
            http::header::USER_AGENT,
            http::HeaderValue::from_bytes(&[0xff, 0xfe]).expect("header"),
        );
        let _span = root_span_on_request_start(&request);
    });

    assert_eq!(fields.get("http_user_agent").as_deref(), Some("[non-UTF8]"));
}

/// `request_id` stays empty unless an upstream `TraceLayer` put one in the
/// extensions — this crate never generates one of its own.
#[test]
fn records_the_request_id_only_when_present() {
    let fields = capture(|| {
        let request = http::Request::builder().uri("/").body(()).expect("request");
        let _span = root_span_on_request_start(&request);
    });
    assert_eq!(fields.get("request_id"), None);

    let id = treat_tower::RequestId::from(uuid::Uuid::nil());
    let fields = capture(|| {
        let mut request = http::Request::builder().uri("/").body(()).expect("request");
        request.extensions_mut().insert(id);
        let _span = root_span_on_request_start(&request);
    });
    assert_eq!(fields.get("request_id").as_deref(), Some(id.to_string().as_str()));
}

#[test]
fn records_the_status_and_exact_body_size() {
    let fields = capture(|| {
        let request = http::Request::builder().uri("/").body(()).expect("request");
        let span = root_span_on_request_start(&request);

        let response = http::Response::builder()
            .status(http::StatusCode::NOT_FOUND)
            .body(String::from("four-oh-four"))
            .expect("response");
        let response = root_span_on_response_end(&span, response);

        // The response must come back untouched.
        assert_eq!(response.status(), http::StatusCode::NOT_FOUND);
        assert_eq!(response.body(), "four-oh-four");
    });

    assert_eq!(fields.get("http_status_code").as_deref(), Some("404"));
    assert_eq!(fields.get("http_body_size").as_deref(), Some("12"));
}
