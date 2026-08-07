//! Coverage for `ApiError::track` and the `ApiErrorTrack::track_api_error`
//! extension. Both preludes are imported to assert the trait sets don't clash.
#![allow(unused_imports)]

use erris::prelude::*;
use treat::error;
use treat::prelude::*;

#[test]
fn track_adds_a_source_frame() {
    let e = error("e");
    assert!(e.source().is_none());

    let tracked = e.track();
    assert!(tracked.source().is_some());
    assert_eq!(*tracked.code(), "e");
}

#[test]
fn track_api_error_passes_ok_through() {
    let ok: Result<ApiResponse<()>, treat::ApiError> = Ok(success(()));
    assert!(ok.track_api_error().is_ok());
}

#[test]
fn track_api_error_tracks_the_err() {
    let err: Result<ApiResponse<()>, treat::ApiError> = Err(error("e"));
    let tracked = err.track_api_error().expect_err("expected error");
    assert!(tracked.source().is_some());
    assert_eq!(*tracked.code(), "e");
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum Code {
    #[default]
    Internal,
}

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Internal => write!(f, "internal"),
        }
    }
}

#[test]
fn track_api_response_wraps_ok_in_success() {
    let ok: erris::Result<u8> = Ok(7);
    let response: Result<ApiResponse<u8>, treat::ApiError<Code>> = ok.track_api_response();
    assert_eq!(response.expect("ok").inner_data().expect("data"), &7);
}

#[test]
fn track_api_response_defaults_the_code_and_keeps_the_source() {
    let err: erris::Result<u8> = Err(erris::report!("boom"));
    let converted: Result<ApiResponse<u8>, treat::ApiError<Code>> = err.track_api_response();
    let converted = converted.expect_err("defaulted");

    assert_eq!(*converted.code(), Code::Internal);
    assert!(converted.source().is_some(), "the report is kept as the cause");
}

/// Accepts any `IntoReport` error, not just an `erris::Report`.
#[test]
fn track_api_response_accepts_foreign_errors() {
    let err: Result<u8, std::io::Error> = Err(std::io::Error::other("disk"));
    let converted: Result<ApiResponse<u8>, treat::ApiError<Code>> = err.track_api_response();
    let converted = converted.expect_err("defaulted");

    assert_eq!(*converted.code(), Code::Internal);
    assert!(converted.source().is_some());
}

// Reads as a bare call in a handler, which is the documented usage.
#[test]
fn track_api_response_composes_with_question_mark() {
    fn handler(fail: bool) -> Result<ApiResponse<u8>, treat::ApiError<Code>> {
        let value: erris::Result<u8> = if fail { Err(erris::report!("boom")) } else { Ok(1) };
        value.track_api_response()
    }

    assert!(handler(false).is_ok());
    assert_eq!(*handler(true).expect_err("defaulted").code(), Code::Internal);
}
