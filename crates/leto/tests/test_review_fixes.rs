//! Regressions for the API-quality review fixes.

use leto::{error, success};

// ---------------------------------------------------------------------------
// Extractor rejections carry a real status instead of 200.
// ---------------------------------------------------------------------------

#[cfg(feature = "serde-path")]
#[test]
fn a_rejected_body_reports_422_not_200() {
    use leto::{REJECTION_STATUS, deserialize_body};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Body {
        #[allow(dead_code)]
        email: String,
    }

    let err = deserialize_body::<Body>(br#"{"email": 42}"#).expect_err("type mismatch");
    assert_eq!(*err.code(), "invalid_body");
    assert_eq!(err.status(), REJECTION_STATUS);
    assert_eq!(err.status(), 422);
    // The locator must survive alongside the new status.
    assert_eq!(err.error_source().and_then(|s| s.pointer.as_deref()), Some("/email"));
}

#[cfg(all(feature = "serde-path", feature = "axum"))]
#[test]
fn axum_body_rejection_renders_422() {
    use axum::response::IntoResponse;
    use leto::deserialize_body;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Body {
        #[allow(dead_code)]
        email: String,
    }

    let err = deserialize_body::<Body>(br#"{"email": 42}"#).expect_err("type mismatch");
    assert_eq!(err.into_response().status().as_u16(), 422);
}

#[cfg(all(feature = "validator-extract", feature = "actix"))]
#[test]
fn actix_validation_rejection_renders_422_not_200() {
    use actix_web::ResponseError;
    use leto::ValidateApi;
    use leto::extract_actix::ApiResponseError;
    use validator::Validate;

    #[derive(Debug, Validate)]
    struct Body {
        #[validate(email)]
        email: String,
    }

    let envelope = Body { email: "nope".into() }.validate_api().expect_err("invalid");
    let rejection = ApiResponseError::new(envelope);
    assert_eq!(rejection.status_code().as_u16(), 422);
    // Still overridable for callers who want the historical 200.
    assert_eq!(rejection.with_status(200).status_code().as_u16(), 200);
}

// ---------------------------------------------------------------------------
// Verbose mode must not repeat each cause once per ancestor.
// ---------------------------------------------------------------------------

#[test]
fn verbose_chain_does_not_duplicate_cause_text() {
    let inner = error("inner").with_message("im");
    let outer = error("outer").with_message("om").with_error(inner).with_verbose();

    let response = outer.into_api_response::<()>();
    let messages: Vec<Option<&str>> = response.errors.iter().map(|e| e.message.as_deref()).collect();

    // Each link contributes its own message verbatim: the chain IS the list.
    assert_eq!(messages, vec![Some("om"), Some("im")]);

    // The `Display` prefix must not leak into the serialized payload.
    let json = serde_json::to_string(&response).expect("serialize");
    assert!(
        !json.contains("leto error:"),
        "internal Display prefix leaked into the payload: {json}"
    );
    // And the inner text appears exactly once, not folded into its parent too.
    assert_eq!(json.matches("\"im\"").count(), 1, "cause duplicated: {json}");
}

// The `verbose-error` feature forces verbose mode crate-wide, so there is no
// non-verbose path to assert when it is on.
#[cfg(not(feature = "verbose-error"))]
#[test]
fn non_verbose_still_reports_only_the_top_error() {
    let outer = error("outer")
        .with_message("om")
        .with_error(error("inner").with_message("im"));
    let response = outer.into_api_response::<()>();
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.first_error_code(), Some("outer"));
}

// `format_message_verbose` still folds the chain in — it feeds logs, where one
// flat line is the point.
#[test]
fn format_message_verbose_still_folds_the_chain_for_logs() {
    let outer = error("outer")
        .with_message("om")
        .with_error(error("inner").with_message("im"));
    let folded = outer.format_message_verbose().expect("message");
    assert!(folded.starts_with("om"), "unexpected: {folded}");
    assert!(folded.contains("im"), "chain missing from log line: {folded}");
}

// ---------------------------------------------------------------------------
// Wrapping drops the inner status by default; `with_source_status` opts in.
// ---------------------------------------------------------------------------

#[test]
fn wrapping_does_not_inherit_the_inner_status_by_default() {
    let inner = error("inner").with_status(404);
    let outer = error("outer").with_error(inner);
    assert_eq!(
        outer.status(),
        leto::DEFAULT_ERROR_STATUS,
        "wrapping must let the outer layer decide",
    );
    assert!(!outer.has_status());
}

#[test]
fn with_source_status_carries_a_nested_status_outward() {
    let inner = error("inner").with_status(404);
    let inner_status = inner.status();
    let outer = error("outer").with_error(inner).with_source_status(inner_status);
    assert_eq!(outer.status(), 404);
    assert!(outer.has_status());
}

#[test]
fn with_source_status_never_overrides_an_explicit_status() {
    let outer = error("outer").with_status(410).with_source_status(404);
    assert_eq!(outer.status(), 410, "an explicit status must win");
}

// ---------------------------------------------------------------------------
// The `bool` guard yields `()` — there is no informative value to return.
// ---------------------------------------------------------------------------

#[test]
fn bool_guard_yields_unit() {
    use leto::OkOrError;

    let allowed: Result<(), _> = true.ok_or_api_error("forbidden");
    assert_eq!(allowed.expect("allowed"), ());

    let denied = false.ok_or_api_error("forbidden").expect_err("denied");
    assert_eq!(*denied.code(), "forbidden");

    let with_message = false
        .ok_or_api_error_with_message("forbidden", "not allowed")
        .expect_err("denied");
    assert_eq!(
        with_message.message().map(|m| m.to_string()),
        Some("not allowed".into())
    );
}

// Reads as a bare guard in a handler, which is the documented usage.
#[test]
fn bool_guard_composes_with_question_mark() {
    use leto::{ApiError, ApiResponse, OkOrError};

    fn handler(is_owner: bool) -> Result<ApiResponse<u8>, ApiError> {
        is_owner.ok_or_api_error("forbidden")?;
        Ok(success(1))
    }

    assert!(handler(true).is_ok());
    assert_eq!(*handler(false).expect_err("denied").code(), "forbidden");
}
