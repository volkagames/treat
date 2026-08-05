//! The tower `TraceLayer` installs a `RequestId` into the request extensions,
//! adopts a caller-supplied `x-request-id`, and echoes it back on the response.
//! Driven through axum's router so the tower `Service` wiring is exercised end
//! to end.

use axum::Router;
use axum::body::Body;
use axum::extract::Extension;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use tower::ServiceExt; // for `oneshot`
use treat_tower::{RequestId, TraceLayer, X_REQUEST_ID};

async fn handler(Extension(id): Extension<RequestId>) -> String {
    // Prove the middleware put a RequestId into the request extensions and that
    // it reached the handler.
    id.to_string()
}

/// Sends one request through the layer, returning the id the handler saw and the
/// id echoed on the response header.
async fn call_with(header: Option<&str>) -> (String, String) {
    let app = Router::new().route("/", get(handler)).layer(TraceLayer::new());

    let mut request = Request::builder().uri("/");
    if let Some(header) = header {
        request = request.header(X_REQUEST_ID, header);
    }

    let response = app
        .oneshot(request.body(Body::empty()).expect("request"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);

    let echoed = response
        .headers()
        .get(X_REQUEST_ID)
        .expect("the layer must echo the request id")
        .to_str()
        .expect("ascii header")
        .to_owned();

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let seen = String::from_utf8(bytes.to_vec()).expect("utf8");

    (seen, echoed)
}

#[tokio::test]
async fn layer_injects_request_id_and_preserves_response() {
    let app = Router::new().route("/", get(handler)).layer(TraceLayer::new());

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).expect("request"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let id = String::from_utf8(bytes.to_vec()).expect("utf8");
    // A v4 UUID renders as 36 chars; just assert the handler saw a non-empty id.
    assert_eq!(id.len(), 36);
}

#[tokio::test]
async fn each_request_gets_a_distinct_id() {
    let app = Router::new().route("/", get(handler)).layer(TraceLayer::new());

    let call = |app: Router| {
        async move {
            let resp = app
                .oneshot(Request::builder().uri("/").body(Body::empty()).expect("request"))
                .await
                .expect("response");
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.expect("body");
            String::from_utf8(bytes.to_vec()).expect("utf8")
        }
    };

    let first = call(app.clone()).await;
    let second = call(app).await;
    assert_ne!(first, second);
}

#[tokio::test]
async fn generated_id_is_echoed_on_the_response() {
    let (seen, echoed) = call_with(None).await;

    assert_eq!(seen.len(), 36, "a v4 UUID renders as 36 chars");
    assert_eq!(echoed, seen, "the echoed header must match the handler's id");
}

#[tokio::test]
async fn caller_supplied_uuid_is_adopted() {
    let supplied = "67e55044-10b1-426f-9247-bb680e5fe0c8";

    let (seen, echoed) = call_with(Some(supplied)).await;

    assert_eq!(seen, supplied, "the caller's id must reach the handler");
    assert_eq!(echoed, supplied, "a canonical id is echoed back byte for byte");
}

/// A non-canonical UUID is adopted but re-emitted canonically. Normalizing is the
/// point: every service on the path then logs and forwards the same lowercase
/// hyphenated string, so one id format holds end to end.
#[tokio::test]
async fn non_canonical_uuid_is_normalized() {
    const CANONICAL: &str = "67e55044-10b1-426f-9247-bb680e5fe0c8";

    for supplied in [
        "{67e55044-10b1-426f-9247-bb680e5fe0c8}",
        "urn:uuid:67e55044-10b1-426f-9247-bb680e5fe0c8",
        "67e5504410b1426f9247bb680e5fe0c8",
        "67E55044-10B1-426F-9247-BB680E5FE0C8",
    ] {
        let (seen, echoed) = call_with(Some(supplied)).await;

        assert_eq!(seen, CANONICAL, "{supplied:?} must reach the handler canonically");
        assert_eq!(echoed, CANONICAL, "{supplied:?} must be echoed canonically");
    }
}

/// Only a UUID is adopted. A non-UUID header must not reach the span or the
/// response, otherwise a caller could inject arbitrary text into every log line
/// that records `request_id`.
#[tokio::test]
async fn non_uuid_header_is_ignored() {
    for supplied in ["not-a-uuid", "", "   ", "'; DROP TABLE users --"] {
        let (seen, echoed) = call_with(Some(supplied)).await;

        assert_ne!(seen, supplied, "{supplied:?} must not be adopted");
        assert_eq!(seen.len(), 36, "a fresh UUID replaces {supplied:?}");
        assert_eq!(echoed, seen);
    }
}

/// Padding is tolerated, since proxies commonly add it.
#[tokio::test]
async fn surrounding_whitespace_is_trimmed() {
    let (seen, _) = call_with(Some("  67e55044-10b1-426f-9247-bb680e5fe0c8  ")).await;

    assert_eq!(seen, "67e55044-10b1-426f-9247-bb680e5fe0c8");
}

/// A handler that sets the header itself owns the value; the layer must not
/// overwrite it.
#[tokio::test]
async fn handler_set_header_is_preserved() {
    async fn sets_header() -> impl axum::response::IntoResponse {
        ([(X_REQUEST_ID, "chosen-by-handler")], "body")
    }

    let app = Router::new().route("/", get(sets_header)).layer(TraceLayer::new());

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).expect("request"))
        .await
        .expect("response");

    let values: Vec<_> = response.headers().get_all(X_REQUEST_ID).iter().collect();
    assert_eq!(values.len(), 1, "the layer must not append a second value");
    assert_eq!(values[0], "chosen-by-handler");
}
