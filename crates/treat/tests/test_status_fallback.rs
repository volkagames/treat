//! A bogus transport status is a bug in the *calling* code, not a different
//! kind of failure — so it must not change what the response means.
//!
//! `resolve_status` falls back to [`DEFAULT_ERROR_STATUS`] rather than a `5xx`:
//! HTTP is the transport, it delivered the response, and the refusal itself
//! lives in `errors[]`. A `5xx` would be indistinguishable from a proxy or
//! load-balancer failure (which can drop the envelope) and would trip retry
//! policies and alerting for a valid, fully-delivered answer. The bug is caught
//! by `debug_assert!` in the setters instead.

use treat::{ApiErrorStatus, DEFAULT_ERROR_STATUS, error, resolve_status};

/// Mirrors the private `INVALID_STATUS_FALLBACK`. Unlike [`DEFAULT_ERROR_STATUS`]
/// this is *not* moved by `error-status-500`: it covers a bug in the calling code,
/// not a choice about how failures are reported.
const INVALID_STATUS_FALLBACK: u16 = 200;

#[test]
fn resolve_status_passes_through_valid_codes() {
    for status in [100, 200, 404, 422, 500, 599] {
        assert_eq!(resolve_status(status), status, "status {status} must pass through");
    }
}

#[test]
fn resolve_status_falls_back_for_invalid_codes() {
    for status in [0, 1, 99, 1000, u16::MAX] {
        assert_eq!(
            resolve_status(status),
            INVALID_STATUS_FALLBACK,
            "invalid status {status} must fall back to {INVALID_STATUS_FALLBACK}",
        );
    }
}

// `http::StatusCode` accepts `100..=999` — it only rejects what it cannot
// represent. A `6xx`–`9xx` code is representable but belongs to no HTTP class,
// so it would reach the client as `<unknown status code>`. `resolve_status` is
// deliberately stricter than the `http` crate and treats those as caller bugs.
#[test]
fn resolve_status_rejects_codes_above_the_5xx_class() {
    for status in [600, 700, 799, 999] {
        assert_eq!(
            resolve_status(status),
            INVALID_STATUS_FALLBACK,
            "status {status} is representable but classless; it must not reach the wire",
        );
    }
}

#[test]
fn the_invalid_status_fallback_is_never_a_server_error() {
    // A caller-side bug must not masquerade as an infrastructure failure: a 5xx
    // is what a proxy emits when it loses the response body entirely. This holds
    // regardless of `error-status-500`, which only moves the unset-status default.
    assert!(
        !(500..600).contains(&resolve_status(u16::MAX)),
        "the invalid-status fallback must never be a 5xx",
    );
}

// `with_status` / `with_code_status` assert in debug builds, so the adapter-level
// fallback is only reachable in release. Exercise that path by driving
// `resolve_status` the way each adapter does.
#[test]
fn adapters_share_one_resolution_rule() {
    let valid = error("boom").with_status(404);
    assert_eq!(resolve_status(valid.status()), 404);

    // An unset status resolves to the configured default.
    assert_eq!(resolve_status(error("boom").status()), DEFAULT_ERROR_STATUS);
}

#[test]
#[should_panic(expected = "invalid HTTP status")]
fn with_status_rejects_an_out_of_range_value_in_debug() {
    let _ = error("boom").with_status(9999);
}

#[test]
#[should_panic(expected = "invalid HTTP status")]
fn with_code_status_rejects_a_bogus_mapping_in_debug() {
    #[derive(Debug, Clone)]
    struct Code;
    impl std::fmt::Display for Code {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "bogus")
        }
    }
    impl ApiErrorStatus for Code {
        fn status_code(&self) -> u16 {
            9999
        }
    }
    let _ = error(Code).with_code_status();
}

#[cfg(feature = "actix")]
#[test]
fn actix_maps_a_valid_status_faithfully() {
    use actix_web::ResponseError;
    assert_eq!(error("boom").with_status(404).status_code().as_u16(), 404);
    assert_eq!(error("boom").status_code().as_u16(), DEFAULT_ERROR_STATUS);
}

#[cfg(feature = "axum")]
#[test]
fn axum_maps_a_valid_status_faithfully() {
    use axum::response::IntoResponse;
    assert_eq!(error("boom").with_status(422).into_response().status().as_u16(), 422);
}

#[cfg(feature = "poem")]
#[test]
fn poem_status_and_as_response_agree() {
    use poem::error::ResponseError;
    let err = error("boom").with_status(404);
    // Regression: `as_response` used to recompute the status independently of
    // `status()`; the two must not drift.
    assert_eq!(ResponseError::status(&err).as_u16(), 404);
    assert_eq!(err.as_response().status().as_u16(), 404);
}
