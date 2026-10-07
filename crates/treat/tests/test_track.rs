//! Coverage for `ApiError::track` and the `ApiErrorTrack::track_api_error`
//! extension. Both preludes are imported to assert the trait sets don't clash.
// Std `Result` coverage: with erris `tracked` on, `erris::Result` is `TrackedResult`
// (see `test_tracked_wrap.rs`), so these don't compile there.
#![cfg(not(feature = "tracked"))]
#![allow(unused_imports)]

use erris::prelude::*;
use treat::error;
use treat::prelude::*;

#[test]
fn track_records_a_hop() {
    let e = error("e");
    assert!(e.hops().is_empty());

    let line = line!() + 1;
    let tracked = e.track();
    assert_eq!(tracked.hops().iter().map(|hop| hop.line()).collect::<Vec<_>>(), [line]);
    assert_eq!(*tracked.code(), "e");
}

// A hop is not a cause: a business error stays source-less, so `error_log`
// keeps it at `debug!` (#1).
#[test]
fn track_does_not_give_a_plain_error_a_source() {
    let e = error("user_not_found").with_message("no such user").track();
    assert!(e.source().is_none());
    assert_eq!(e.format_message_verbose().as_deref(), Some("no such user"));
}

// The source still renders after a hop, so the verbose message keeps its
// cause and no dangling separator (#1).
#[test]
fn track_keeps_the_source_rendering() {
    let e = error("ticket_send_error")
        .with_message("unable send ticket to zendesk service")
        .with_source(erris::report!("inner").with_err(erris::report!("unable create new ticket")));
    let before = e.format_message_verbose();

    let e = e.track();
    assert_eq!(e.format_message_verbose(), before);
    assert_eq!(
        e.format_message_verbose().as_deref(),
        Some("unable send ticket to zendesk service, unable create new ticket")
    );
    assert_eq!(
        e.source().map(ToString::to_string).as_deref(),
        Some("unable create new ticket")
    );
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
    assert_eq!(tracked.hops().len(), 1);
    assert!(tracked.source().is_none());
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

// `Debug` shows the path the error travelled: hops outermost first, then the
// raise site, then the source chain.
#[test]
fn debug_lists_hops_above_the_raise_site() {
    let raise = line!() + 1;
    let e = error("e").with_message("m").with_error(erris::report!("cause"));
    let hop = line!() + 1;
    let e = e.track();

    let debug = format!("{e:?}");
    assert!(
        debug.starts_with("treat error: e, message: m\n\nCaused by:\n   0: cause\n\nLocation:"),
        "{debug}"
    );
    let position = |line: u32| {
        debug
            .find(&format!("test_track.rs:{line}:"))
            .unwrap_or_else(|| panic!("line {line} missing: {debug}"))
    };
    assert!(position(hop) < position(raise), "{debug}");
}
