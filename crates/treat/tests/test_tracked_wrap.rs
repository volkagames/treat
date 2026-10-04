//! Coverage for `WrapApiError` and `ApiResponseTrack` on erris's `TrackedResult`
//! (the `tracked` feature, nightly).
//!
//! Run with `cargo +nightly nextest run -p treat --features tracked --test test_tracked_wrap`.
#![cfg(feature = "tracked")]

use erris::tracked::{Err, Ok};
use treat::{ApiError, ApiErrorHandler, ApiResponse, ApiResponseTrack, WrapApiError};

#[derive(Clone, Debug, Default, treat::ApiErrorCode)]
enum Code {
    #[default]
    #[code("internal")]
    #[message("the request could not be completed")]
    #[status(500)]
    Internal,
}

fn failed() -> erris::TrackedResult<u32> {
    Err(erris::report!("boom"))
}

#[test]
fn ok_passes_through() {
    let ok: erris::TrackedResult<u32> = Ok(1);
    assert_eq!(ok.wrap_api_error(Code::Internal).expect("ok"), 1);
}

#[test]
fn wrap_api_error_keeps_the_report_as_source() {
    let e = failed().wrap_api_error(Code::Internal).expect_err("err");
    assert!(matches!(e.code(), Code::Internal));
    assert!(e.message().is_none(), "wrap_api_error does not apply #[message]");
    assert_eq!(e.source().map(ToString::to_string).as_deref(), Some("boom"));
}

#[test]
fn wrap_api_code_applies_the_declared_message() {
    let e = failed().wrap_api_code(Code::Internal).expect_err("err");
    assert_eq!(
        e.message().map(|m| m.as_ref()),
        Some("the request could not be completed")
    );
    assert!(e.source().is_some());

    let e: ApiError<Code> = failed().wrap_api_error_default().expect_err("err");
    assert_eq!(e.status(), 500);
}

#[test]
fn wrap_api_error_and_message_and_with() {
    let e = failed()
        .wrap_api_error_and_message(Code::Internal, "custom")
        .expect_err("err");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("custom"));

    let e = failed()
        .wrap_api_error_with(|| (Code::Internal, "lazy"))
        .expect_err("err");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("lazy"));
}

/// The extra hop through the std impl must not move the location into treat.
#[test]
fn reports_the_caller_location() {
    let expected_line = line!() + 1;
    let e = failed().wrap_api_error(Code::Internal).expect_err("err");

    let location = ApiErrorHandler::location(&e);
    assert_eq!(location.line(), expected_line);
    assert!(
        location.file().ends_with("test_tracked_wrap.rs"),
        "location pointed at {}, not the call site",
        location.file(),
    );
}

#[test]
fn track_api_response_wraps_ok_in_success() {
    let ok: erris::TrackedResult<u32> = Ok(7);
    let response: Result<ApiResponse<u32>, ApiError<Code>> = ok.track_api_response();
    assert_eq!(response.expect("ok").into_inner_data().into_std().expect("data"), 7);
}

#[test]
fn track_api_response_defaults_the_code_and_keeps_the_source() {
    let converted: Result<ApiResponse<u32>, ApiError<Code>> = failed().track_api_response();
    let e = converted.expect_err("defaulted");
    assert!(matches!(e.code(), Code::Internal));
    assert_eq!(e.source().map(ToString::to_string).as_deref(), Some("boom"));
}
