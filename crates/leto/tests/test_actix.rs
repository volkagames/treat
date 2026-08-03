//! Coverage for the actix-web adapters: `Responder` for `ApiResponse` and
//! `ResponseError` for `ApiError`.
#![cfg(feature = "actix")]

use actix_web::body::MessageBody;
use actix_web::http::StatusCode;
use actix_web::{HttpResponse, Responder, ResponseError};
use leto::{ApiResponse, DEFAULT_ERROR_STATUS, error, success};

fn read_json<B, T>(resp: HttpResponse<B>) -> T
where
    B: MessageBody,
    T: serde::de::DeserializeOwned,
{
    let bytes = match resp.into_body().try_into_bytes() {
        Ok(bytes) => bytes,
        Err(_) => panic!("response body should be fully buffered"),
    };
    serde_json::from_slice(&bytes).expect("response body should be valid leto json")
}

#[test]
fn responder_serializes_a_success_envelope() {
    let req = actix_web::test::TestRequest::default().to_http_request();
    let resp = success(7_i32).respond_to(&req);
    assert_eq!(resp.status(), StatusCode::OK);

    let decoded: ApiResponse<i32> = read_json(resp);
    assert_eq!(decoded.data, Some(7));
    assert!(decoded.errors.is_empty());
}

#[test]
fn response_error_returns_the_default_status_with_error_body() {
    let e = error("boom");
    // Errors are carried in the body; the HTTP status is the configured default
    // (200, or 500 under `error-status-500`).
    let default_status = StatusCode::from_u16(DEFAULT_ERROR_STATUS).expect("valid default status");
    assert_eq!(e.status_code(), default_status);

    let resp = e.error_response();
    assert_eq!(resp.status(), default_status);

    let decoded: ApiResponse = read_json(resp);
    assert_eq!(decoded.first_error_code(), Some("boom"));
    assert!(decoded.data.is_none());
}

#[test]
fn with_status_sets_the_response_status() {
    let e = error("not_found").with_status(404);
    assert_eq!(e.status_code(), StatusCode::NOT_FOUND);

    let resp = e.error_response();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // The status is transport-only: the body still carries the error and no data.
    let decoded: ApiResponse = read_json(resp);
    assert_eq!(decoded.first_error_code(), Some("not_found"));
    assert!(decoded.data.is_none());
}

// The Responder path also handles `Result<ApiResponse, ApiError>` via actix.
#[cfg(feature = "derive")]
#[test]
fn error_message_roundtrips_into_a_typed_error() {
    use leto::{ErrorMessage, FromErrorMessage};
    use thiserror::Error;

    #[derive(Error, FromErrorMessage, Debug, PartialEq)]
    enum SomeApiError {
        #[error("cool error")]
        #[code("friend_invite_not_exist")]
        CoolError(ErrorMessage),
        #[error("other error")]
        #[code("_")]
        Other(ErrorMessage),
    }

    let req = actix_web::test::TestRequest::default().to_http_request();
    let resp: Result<ApiResponse<()>, _> = error("friend_invite_not_exist").with_message("some message").err();
    let http_response = resp.respond_to(&req);

    let decoded: ApiResponse = read_json(http_response);
    assert_eq!(decoded.errors.len(), 1);
    let err = decoded.err().expect("error");
    assert_eq!(err.code, "friend_invite_not_exist");
    assert_eq!(err.message.as_deref(), Some("some message"));

    let typed: SomeApiError = err.into();
    assert!(matches!(typed, SomeApiError::CoolError(_)));
}
