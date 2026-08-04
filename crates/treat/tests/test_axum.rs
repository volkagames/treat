//! Coverage for the axum adapters: `IntoResponse` for `ApiResponse`/`ApiError`
//! and the `response_get_api_error` extension helper.
#![cfg(feature = "axum")]

use axum::response::IntoResponse;
use treat::{ApiResponse, DEFAULT_ERROR_STATUS, error, response_get_api_error, success};

#[test]
fn api_error_into_response_stashes_a_handler_extension() {
    let resp = error("access_denied").with_message("nope").into_response();
    assert_eq!(resp.status().as_u16(), DEFAULT_ERROR_STATUS);

    let handler = response_get_api_error(&resp).expect("api error handler in extensions");
    assert_eq!(handler.code(), "access_denied");
    assert_eq!(handler.to_error_message().code, "access_denied");
}

#[test]
fn api_response_into_response_is_200() {
    let resp = success(1_i32).into_response();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);
}

#[test]
fn with_status_sets_the_response_status() {
    let resp = error("not_found").with_status(404).into_response();
    assert_eq!(resp.status(), axum::http::StatusCode::NOT_FOUND);

    // The stashed handler still exposes the error regardless of status.
    let handler = response_get_api_error(&resp).expect("api error handler in extensions");
    assert_eq!(handler.code(), "not_found");
}

#[tokio::test]
async fn api_error_body_is_a_treat_envelope() {
    let resp = error("boom").into_response();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body should buffer");
    let decoded: ApiResponse = serde_json::from_slice(&bytes).expect("valid treat json");
    assert_eq!(decoded.first_error_code(), Some("boom"));
}
