//! The `error-status-500` feature flips the *unset* transport status only.
//!
//! Without it an error travels in the `errors[]` envelope with `200 OK`. With it
//! the same error reports `500`, so clients that only read the status line — and
//! monitoring that alerts on `5xx` — see the failure. An explicitly set status
//! and the invalid-status fallback are unaffected either way.

use leto::{ApiResponse, DEFAULT_ERROR_STATUS, error, resolve_status};

#[cfg(not(feature = "error-status-500"))]
#[test]
fn the_default_is_200_without_the_feature() {
    assert_eq!(DEFAULT_ERROR_STATUS, 200);
    assert_eq!(error("boom").status(), 200);
}

#[cfg(feature = "error-status-500")]
#[test]
fn the_default_is_500_with_the_feature() {
    assert_eq!(DEFAULT_ERROR_STATUS, 500);
    assert_eq!(error("boom").status(), 500);
}

/// The point of the flag: it must not silently override a status the caller chose.
#[test]
fn an_explicit_status_wins_over_the_default() {
    assert_eq!(error("not_found").with_status(404).status(), 404);
    assert_eq!(error("teapot").with_status(418).status(), 418);
}

/// `has_status` distinguishes "defaulted" from "set", and must keep doing so when
/// the default happens to equal a status a caller might set explicitly.
#[test]
fn the_default_is_still_reported_as_unset() {
    assert!(!error("boom").has_status());
    assert!(error("boom").with_status(DEFAULT_ERROR_STATUS).has_status());
}

/// The invalid-status fallback is a caller-bug safety net, not a reporting policy:
/// it stays a non-5xx even when the feature moves the default to 500.
#[test]
fn the_feature_does_not_move_the_invalid_status_fallback() {
    assert_eq!(resolve_status(u16::MAX), 200);
    assert_eq!(resolve_status(0), 200);
}

/// The status is transport-only: flipping the default must not change the body.
#[test]
fn the_envelope_body_is_unchanged_by_the_flag() {
    let decoded: ApiResponse = error("boom").with_message("bad").into_api_response();
    assert_eq!(decoded.first_error_code(), Some("boom"));
    assert!(decoded.data.is_none());
}

#[cfg(all(feature = "axum", feature = "error-status-500"))]
#[test]
fn axum_returns_500_for_an_unset_status() {
    use axum::response::IntoResponse;
    assert_eq!(error("boom").into_response().status().as_u16(), 500);
    // Success responses are untouched.
    assert_eq!(leto::success(1_i32).into_response().status().as_u16(), 200);
}

#[cfg(all(feature = "actix", feature = "error-status-500"))]
#[test]
fn actix_returns_500_for_an_unset_status() {
    use actix_web::ResponseError;
    assert_eq!(error("boom").error_response().status().as_u16(), 500);
}

#[cfg(all(feature = "poem", feature = "error-status-500"))]
#[test]
fn poem_returns_500_for_an_unset_status() {
    use poem::error::ResponseError;
    let err = error("boom");
    assert_eq!(ResponseError::status(&err).as_u16(), 500);
    assert_eq!(err.as_response().status().as_u16(), 500);
}
