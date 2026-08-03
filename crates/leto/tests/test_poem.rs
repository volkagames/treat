//! Coverage for the poem adapters: `IntoResponse` for `ApiResponse` and
//! `ResponseError` for `ApiError`, plus that the wire format matches the shared
//! `into_api_response` builder (so poem agrees with actix and axum).
#![cfg(feature = "poem")]

use leto::{ApiResponse, DEFAULT_ERROR_STATUS, error, success};
use poem::IntoResponse;
use poem::error::ResponseError;
use poem::http::StatusCode;

#[tokio::test]
async fn api_error_as_response_is_a_leto_envelope() {
    let err = error("boom").with_message("bad");
    let default_status = StatusCode::from_u16(DEFAULT_ERROR_STATUS).expect("valid default status");
    assert_eq!(ResponseError::status(&err), default_status);

    let resp = err.as_response();
    assert_eq!(resp.status(), default_status);

    let body = resp.into_body().into_vec().await.expect("buffer body");
    let decoded: ApiResponse = serde_json::from_slice(&body).expect("valid leto json");
    assert_eq!(decoded.first_error_code(), Some("boom"));
}

#[tokio::test]
async fn poem_envelope_matches_canonical_builder() {
    let err = error("boom").with_message("bad").with_pointer("/data/attributes/x");
    let canonical: ApiResponse = err.into_api_response();

    let body = err.as_response().into_body().into_vec().await.expect("buffer body");
    let decoded: ApiResponse = serde_json::from_slice(&body).expect("valid leto json");
    assert_eq!(decoded, canonical);
}

#[tokio::test]
async fn api_response_into_response_is_200() {
    let resp = success(1_i32).into_response();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn with_status_sets_the_response_status() {
    let err = error("not_found").with_status(404);
    assert_eq!(ResponseError::status(&err), StatusCode::NOT_FOUND);

    let resp = err.as_response();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[test]
fn api_error_converts_into_poem_error() {
    // The blanket `From<ResponseError>` lets `?` bubble an `ApiError` from a poem handler.
    let poem_err: poem::Error = error("nope").into();
    assert_eq!(poem_err.status().as_u16(), DEFAULT_ERROR_STATUS);
}
