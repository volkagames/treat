//! Unit coverage for the `ApiError` type: constructors, builders, accessors,
//! message formatting, verbose behaviour, `Display`/`Debug` and `std::error::Error`.

use serde_json::json;
use std::error::Error;
use treat::{ApiError, error, error_and_message, wrap_error};

#[test]
fn constructors_set_expected_fields() {
    let e = error("code_a");
    assert_eq!(*e.code(), "code_a");
    assert!(e.message().is_none());
    assert!(e.meta().is_none());
    assert!(e.source().is_none());

    let e = error_and_message("code_b", "boom");
    assert_eq!(*e.code(), "code_b");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("boom"));
    assert!(e.source().is_none());

    let e = wrap_error(erris::report!("io failed"), "code_c", "context");
    assert_eq!(*e.code(), "code_c");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("context"));
    assert!(e.source().is_some());
}

#[test]
fn builders_are_chainable() {
    let e = error("c").with_message("m").with_meta(json!({ "field": "email" }));

    assert_eq!(e.message().map(|m| m.as_ref()), Some("m"));
    assert_eq!(e.meta(), Some(&json!({ "field": "email" })));
}

#[test]
fn with_source_accumulates_into_a_chain() {
    let e = error("c")
        .with_source(erris::report!("first"))
        .with_source(erris::report!("second"));

    let rendered = format!("{:?}", e);
    assert!(rendered.contains("first"), "chain lost first cause: {rendered}");
    assert!(rendered.contains("second"), "chain lost second cause: {rendered}");
}

#[test]
fn with_error_is_a_shortcut_for_with_source() {
    let e = error("c").with_error(erris::report!("cause"));
    assert!(e.source().is_some());
}

#[test]
fn to_error_message_carries_code_message_and_meta() {
    let e = error("c").with_message("m").with_meta(json!({ "x": 1 }));
    let msg = e.to_error_message();
    assert_eq!(msg.code, "c");
    assert_eq!(msg.message.as_deref(), Some("m"));
    assert_eq!(msg.meta, Some(json!({ "x": 1 })));
}

#[test]
fn into_error_message_consumes_self() {
    let msg = error("c").with_message("m").into_error_message();
    assert_eq!(msg.code, "c");
    assert_eq!(msg.message.as_deref(), Some("m"));
}

#[test]
fn verbose_message_merges_message_and_source() {
    let verbose = error("c")
        .with_message("top")
        .with_error(erris::report!("cause"))
        .with_verbose();
    assert!(verbose.is_verbose());
    let msg = verbose.format_message_verbose().expect("verbose message");
    assert!(msg.contains("top"), "missing message: {msg}");
    assert!(msg.contains("cause"), "missing source: {msg}");
}

#[cfg(not(feature = "verbose-error"))]
#[test]
fn non_verbose_message_hides_the_source() {
    let e = error("c").with_message("top").with_error(erris::report!("cause"));
    assert!(!e.is_verbose());
    assert_eq!(e.format_message().as_deref(), Some("top"));
}

#[test]
fn display_renders_code_and_message() {
    let e = error("c").with_message("m");
    assert_eq!(e.to_string(), "treat error: c, message: m");

    let e = error("c");
    assert_eq!(e.to_string(), "treat error: c");
}

#[test]
fn std_error_source_reflects_the_report() {
    let e = error("c").with_error(erris::report!("cause"));
    assert!(Error::source(&e).is_some());

    let e = error("c");
    assert!(Error::source(&e).is_none());
}

#[test]
fn err_and_into_result_wrap_into_err() {
    let r = error("c").err::<()>();
    assert!(r.is_err());
    let r = error("c").into_result::<()>();
    assert!(r.is_err());
}

#[test]
fn clone_preserves_content() {
    let e = error("c").with_message("m").with_error(erris::report!("cause"));
    let cloned = e.clone();
    assert_eq!(e.to_string(), cloned.to_string());
    assert_eq!(e.to_error_message(), cloned.to_error_message());
    assert_eq!(e.collect_messages(), cloned.collect_messages());
}

// `into_api_response` is the single builder shared by every framework adapter.
#[cfg(not(feature = "verbose-error"))]
#[test]
fn response_is_top_only_by_default() {
    let e = error("outer").with_source(erris::report!(error("inner")));
    let resp = e.into_api_response::<()>();
    assert_eq!(resp.errors.len(), 1);
    assert_eq!(resp.errors[0].code, "outer");
}

// A cause that is NOT an `ApiError` carries no `code`, so it cannot become its
// own `errors[]` entry — it has to reach the client merged into the top-level
// message. This is the common shape (`wrap_api_error`, `?` on a foreign error);
// before the fix it was dropped outright and verbose mode was a silent no-op.
#[test]
fn verbose_response_keeps_a_plain_report_cause() {
    let e = error("db_fail")
        .with_error(erris::report!("connection refused"))
        .with_verbose();
    let resp = e.into_api_response::<()>();
    let message = resp.errors[0].message.as_deref().unwrap_or_default();
    assert!(message.contains("connection refused"), "cause dropped: {message}");
}

#[test]
fn verbose_response_keeps_a_wrapped_foreign_error() {
    use treat::prelude::*;

    let e = "abc"
        .parse::<u16>()
        .wrap_api_error("bad_port")
        .expect_err("parse fails");
    let resp = e.with_verbose().into_api_response::<()>();
    let message = resp.errors[0].message.as_deref().unwrap_or_default();
    assert!(message.contains("invalid digit"), "cause dropped: {message}");
}

// A chain holding BOTH kinds: the `ApiError` becomes its own entry, the foreign
// leaf folds into the entry it hangs off. Neither may be dropped.
#[test]
fn verbose_response_keeps_a_mixed_chain() {
    let e = error("outer")
        .with_message("om")
        .with_error(erris::report!("foreign boom"))
        .with_error(erris::report!(error("inner").with_message("im")))
        .with_verbose();
    let resp = e.into_api_response::<()>();
    let json = serde_json::to_string(&resp).expect("serialize");
    assert!(json.contains("foreign boom"), "foreign cause dropped: {json}");
    assert!(json.contains("im"), "ApiError cause dropped: {json}");
    // Folding must never splice the internal `Display` prefix into the payload.
    assert!(!json.contains("treat error:"), "internal prefix leaked: {json}");
}

// `track()` inserts transparent wrappers that render as empty text; they must
// not pad messages with stray separators.
#[test]
fn verbose_response_ignores_transparent_track_wrappers() {
    let e = error("db").with_message("m").track().track().with_verbose();
    let resp = e.into_api_response::<()>();
    assert_eq!(resp.errors[0].message.as_deref(), Some("m"));
}

// The non-verbose default must stay quiet: causes belong in the logs, not the
// payload. Guards the fix above from leaking internals by default.
#[cfg(not(feature = "verbose-error"))]
#[test]
fn non_verbose_response_hides_a_plain_report_cause() {
    let e = error("db_fail").with_error(erris::report!("connection refused"));
    let resp = e.into_api_response::<()>();
    assert_eq!(resp.errors.len(), 1);
    assert!(resp.errors[0].message.is_none(), "cause leaked: {:?}", resp.errors[0]);
}

// An `ApiError` cause is listed as its own entry. It is also merged into the
// top-level message, but it must not appear twice in `errors[]`.
#[test]
fn verbose_response_does_not_duplicate_an_api_error_cause() {
    let e = error("outer")
        .with_message("outer msg")
        .with_source(erris::report!(error("inner").with_message("inner msg")))
        .with_verbose();
    let resp = e.into_api_response::<()>();
    assert_eq!(resp.errors.len(), 2, "duplicated chain: {:?}", resp.errors);
    assert_eq!(resp.errors[0].code, "outer");
    assert_eq!(resp.errors[1].code, "inner");
    // The nested entry keeps its own message verbatim, not a merged one.
    assert_eq!(resp.errors[1].message.as_deref(), Some("inner msg"));
}

#[test]
fn verbose_response_includes_the_full_chain() {
    let e = error("outer")
        .with_source(erris::report!(error("inner")))
        .with_verbose();
    let resp = e.into_api_response::<()>();
    assert!(resp.has_error_code("outer").is_some());
    assert!(resp.has_error_code("inner").is_some());
}

#[test]
fn collect_messages_always_returns_the_chain() {
    let e = error("outer").with_source(erris::report!(error("inner")));
    let msgs = e.collect_messages();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].code, "outer");
    assert_eq!(msgs[1].code, "inner");
}

// Regression (F1): `collect_messages` must find `ApiError`s in the cause chain
// whose code type is NOT the default `&'static str`. Before the fix the internal
// `downcast_ref::<ApiError>()` defaulted to `ApiError<&'static str>`, so a typed
// code nested as a source was silently dropped.
#[test]
fn collect_messages_includes_typed_code_sources() {
    #[derive(Debug, Clone, PartialEq)]
    struct Code(u16);
    impl std::fmt::Display for Code {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "E{:03}", self.0)
        }
    }

    let inner: ApiError<Code> = error(Code(42));
    let outer: ApiError<Code> = error(Code(7)).with_source(erris::report!(inner));

    let codes: Vec<String> = outer.collect_messages().into_iter().map(|m| m.code).collect();
    assert_eq!(codes, vec!["E007".to_string(), "E042".to_string()]);
}

// Regression (F1): a typed top-level error with a default-`&str`-coded source
// (the common "library helper wrapped in a service error" shape) keeps both.
#[test]
fn collect_messages_mixes_typed_and_str_codes() {
    #[derive(Debug, Clone, PartialEq)]
    struct Code(u16);
    impl std::fmt::Display for Code {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "E{:03}", self.0)
        }
    }

    let leaf = error("io_failed");
    let outer: ApiError<Code> = error(Code(7)).with_source(erris::report!(leaf));

    let codes: Vec<String> = outer.collect_messages().into_iter().map(|m| m.code).collect();
    assert_eq!(codes, vec!["E007".to_string(), "io_failed".to_string()]);
}

// Regression (F8): `to_error_message_with(false)` must honour the argument and
// return the RAW message, even when the error is verbose (via `.with_verbose()`
// or the `verbose-error` feature). Previously the `false` arm called
// `format_message()`, which re-expanded the source under verbose mode.
#[test]
fn to_error_message_with_false_returns_raw_message_when_verbose() {
    let e = error("c")
        .with_message("top")
        .with_error(erris::report!("cause"))
        .with_verbose();
    assert!(e.is_verbose());

    // verbose(true) merges message + source.
    let verbose = e.to_error_message_with(true).message.expect("verbose message");
    assert!(verbose.contains("top"), "verbose lost message: {verbose}");
    assert!(verbose.contains("cause"), "verbose lost source: {verbose}");

    // verbose(false) returns the raw message only — no source expansion.
    let plain = e.to_error_message_with(false);
    assert_eq!(plain.message.as_deref(), Some("top"));
}

// Typed error codes work through the generic parameter, not only `&'static str`.
#[test]
fn typed_error_code() {
    #[derive(Debug, Clone, PartialEq)]
    struct Code(u16);
    impl std::fmt::Display for Code {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "E{:03}", self.0)
        }
    }

    let e: ApiError<Code> = error(Code(42));
    assert_eq!(e.code().0, 42);
    assert_eq!(e.to_error_message().code, "E042");
}

// G1: transport HTTP status. Default is `DEFAULT_ERROR_STATUS` (200, or 500 under
// `error-status-500`); `with_status` overrides; the opt-in `ApiErrorStatus` trait
// seeds it from the code via `with_code_status`.
#[test]
fn status_defaults_to_the_default_and_can_be_set() {
    assert_eq!(error("boom").status(), treat::DEFAULT_ERROR_STATUS);
    assert_eq!(error("boom").with_status(404).status(), 404);
}

#[test]
fn with_code_status_seeds_from_the_code_and_with_status_wins() {
    use treat::ApiErrorStatus;

    #[derive(Debug, Clone)]
    enum Code {
        NotFound,
    }
    impl std::fmt::Display for Code {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "not_found")
        }
    }
    impl ApiErrorStatus for Code {
        fn status_code(&self) -> u16 {
            match self {
                Code::NotFound => 404,
            }
        }
    }

    assert_eq!(error(Code::NotFound).with_code_status().status(), 404);
    // Both setters write the same field, so the last call wins.
    assert_eq!(error(Code::NotFound).with_code_status().with_status(410).status(), 410);
    assert_eq!(error(Code::NotFound).with_status(410).with_code_status().status(), 404);
}
