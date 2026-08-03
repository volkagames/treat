//! The tower `TraceLayer` installs a `RequestId` into the request extensions
//! and passes the response through unchanged. Driven through axum's router so
//! the tower `Service` wiring is exercised end to end.

use axum::Router;
use axum::body::Body;
use axum::extract::Extension;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use leto_tower::{RequestId, TraceLayer};
use tower::ServiceExt; // for `oneshot`

async fn handler(Extension(id): Extension<RequestId>) -> String {
    // Prove the middleware put a RequestId into the request extensions and that
    // it reached the handler.
    id.to_string()
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
