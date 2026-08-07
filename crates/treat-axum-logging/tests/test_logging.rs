//! Coverage for `LoggingLayer`: the layer must hand every `treat` error a
//! response carries to the configured logger, exactly once, and leave the
//! response otherwise untouched.

use axum::response::IntoResponse;
use std::sync::{Arc, Mutex};
use tower::{Layer, Service, ServiceExt};
use treat_axum_logging::LoggingLayer;
use treat_core::{ApiErrorHandler, error};

/// Collects the codes the layer logs, so a test can assert on them.
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<String>>>);

impl Recorder {
    fn logger(&self) -> impl Fn(&dyn ApiErrorHandler) + Clone + Send + 'static {
        let seen = Arc::clone(&self.0);
        move |err| {
            seen.lock().expect("recorder poisoned").push(err.code().to_string());
        }
    }

    fn seen(&self) -> Vec<String> {
        self.0.lock().expect("recorder poisoned").clone()
    }
}

/// Drives one request through the layer and returns the response status.
async fn run<F>(recorder: &Recorder, handler: F) -> http::StatusCode
where
    F: Fn() -> axum::response::Response + Clone + Send + 'static,
{
    let service = tower::service_fn(move |_req: http::Request<axum::body::Body>| {
        let handler = handler.clone();
        async move { Ok::<_, std::convert::Infallible>(handler()) }
    });

    let mut service = LoggingLayer::with_error_logger(recorder.logger()).layer(service);
    let request = http::Request::builder()
        .uri("/")
        .body(axum::body::Body::empty())
        .expect("request");

    let response = service.ready().await.expect("ready").call(request).await.expect("call");
    response.status()
}

#[tokio::test]
async fn logs_the_error_a_response_carries() {
    let recorder = Recorder::default();
    run(&recorder, || error("user_not_found").into_response()).await;

    assert_eq!(recorder.seen(), vec!["user_not_found".to_string()]);
}

#[tokio::test]
async fn leaves_a_plain_response_alone() {
    let recorder = Recorder::default();
    let status = run(&recorder, || "ok".into_response()).await;

    assert_eq!(status, http::StatusCode::OK);
    assert!(recorder.seen().is_empty(), "nothing to log without an ApiError");
}

/// The error travels in the body envelope, so the status line stays whatever the
/// adapter resolved — the layer must not rewrite it.
#[tokio::test]
async fn preserves_the_response_status() {
    let recorder = Recorder::default();
    let status = run(&recorder, || error("teapot").with_status(418).into_response()).await;

    assert_eq!(status, http::StatusCode::IM_A_TEAPOT);
    assert_eq!(recorder.seen(), vec!["teapot".to_string()]);
}

/// Two requests through the same service must log twice — a `Clone`d inner
/// service or a consumed handler would silently drop the second.
#[tokio::test]
async fn logs_every_request() {
    let recorder = Recorder::default();
    run(&recorder, || error("first").into_response()).await;
    run(&recorder, || error("second").into_response()).await;

    assert_eq!(recorder.seen(), vec!["first".to_string(), "second".to_string()]);
}

/// Tracking starts before the inner service runs, so whatever the handler logs
/// is already inside the request span — not just the error at the end.
///
/// Not a `#[tokio::test]`: the subscriber has to stay installed across the whole
/// call, and `with_default`'s guard cannot be held across an `.await`.
#[test]
fn opens_the_root_span_around_the_inner_service() {
    let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let recorded = Arc::clone(&seen);

    let service = tower::service_fn(move |_req: http::Request<axum::body::Body>| {
        let recorded = Arc::clone(&recorded);
        async move {
            // `Span::current()` inside the handler is the span the layer opened.
            recorded
                .lock()
                .expect("poisoned")
                .push(tracing::Span::current().metadata().map(|m| m.name().to_string()));
            Ok::<_, std::convert::Infallible>("ok".into_response())
        }
    });
    let mut service = LoggingLayer::new().layer(service);

    let request = http::Request::builder()
        .uri("/")
        .body(axum::body::Body::empty())
        .expect("request");

    // A span is only materialised while a subscriber is installed.
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::ERROR)
        .finish();
    let runtime = tokio::runtime::Builder::new_current_thread().build().expect("runtime");
    tracing::subscriber::with_default(subscriber, || {
        runtime.block_on(async {
            service.ready().await.expect("ready").call(request).await.expect("call");
        });
    });

    assert_eq!(*seen.lock().expect("poisoned"), vec![Some("request".to_string())]);
}

/// The source chain is what `error_log` keys its severity off, so it must still
/// be reachable through the type-erased handler the layer passes on.
#[tokio::test]
async fn the_logger_sees_the_source_chain() {
    let seen: Arc<Mutex<Vec<bool>>> = Arc::default();
    let recorded = Arc::clone(&seen);

    let service = tower::service_fn(move |_req: http::Request<axum::body::Body>| {
        async move {
            let err = error("internal").with_source(erris::report!("disk"));
            Ok::<_, std::convert::Infallible>(err.into_response())
        }
    });
    let mut service = LoggingLayer::with_error_logger(move |err: &dyn ApiErrorHandler| {
        recorded.lock().expect("poisoned").push(err.source().is_some());
    })
    .layer(service);

    let request = http::Request::builder()
        .uri("/")
        .body(axum::body::Body::empty())
        .expect("request");
    service.ready().await.expect("ready").call(request).await.expect("call");

    assert_eq!(*seen.lock().expect("poisoned"), vec![true]);
}
