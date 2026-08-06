//! Coverage for the `OkOrError` extension trait on `Option` and `bool`.

use treat::prelude::*;
use treat::{ApiError, ApiErrorHandler, error};

/// Exercises the `From<C> for ApiError<C>` route `ok_or_api_code` takes: a
/// variant with a declared `#[message]`, one without, and one whose message
/// interpolates fields.
#[derive(Clone, Debug, treat::ApiErrorCode)]
enum Code {
    #[code("user.not_found")]
    #[message("user was not found")]
    NotFound,

    /// No `#[message]` — the code path must leave the message unset, same as
    /// building the error directly.
    #[code("user.silent")]
    Silent,

    #[code("order.missing")]
    #[message("order {id} is missing")]
    MissingOrder { id: u64 },
}

#[test]
fn option_some_returns_the_value() {
    let some: Option<i32> = Some(5);
    assert_eq!(some.ok_or_api_error("e").expect("some"), 5);

    let some: Option<i32> = Some(5);
    assert_eq!(some.ok_or_api_error_with_message("e", "msg").expect("some"), 5);
}

#[test]
fn option_none_produces_error() {
    let none: Option<i32> = None;
    let e = none.ok_or_api_error("missing").expect_err("none");
    assert_eq!(*e.code(), "missing");
    assert!(e.message().is_none());

    let none: Option<i32> = None;
    let e = none
        .ok_or_api_error_with_message("missing", "no value")
        .expect_err("none");
    assert_eq!(*e.code(), "missing");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("no value"));
}

#[test]
fn bool_true_is_ok_false_is_error() {
    let yes = true;
    assert!(yes.ok_or_api_error("e").is_ok());

    let no = false;
    let e = no.ok_or_api_error("denied").expect_err("false");
    assert_eq!(*e.code(), "denied");

    let no = false;
    let e = no
        .ok_or_api_error_with_message("denied", "not allowed")
        .expect_err("false");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("not allowed"));
}

#[test]
fn option_ok_or_api_code_applies_the_declared_message() {
    let none: Option<u32> = None;
    let e = none.ok_or_api_code(Code::NotFound).expect_err("none");

    assert_eq!(e.code().to_string(), "user.not_found");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("user was not found"));
}

/// The documented contrast between the two constructors: `ok_or_api_error`
/// builds from the code directly and drops `#[message]`, `ok_or_api_code` goes
/// through `From` and keeps it. Same code either way.
#[test]
fn ok_or_api_error_skips_the_declared_message() {
    let none: Option<u32> = None;
    let bare = none.ok_or_api_error(Code::NotFound).expect_err("none");

    let none: Option<u32> = None;
    let via_code = none.ok_or_api_code(Code::NotFound).expect_err("none");

    assert!(bare.message().is_none(), "the direct path must not apply #[message]");
    assert_eq!(via_code.message().map(|m| m.as_ref()), Some("user was not found"));
    assert_eq!(bare.code().to_string(), via_code.code().to_string());
}

/// A code with no `#[message]` must come out message-less even through the
/// `From` route — the impl has nothing to apply.
#[test]
fn ok_or_api_code_leaves_a_message_less_code_bare() {
    let none: Option<u32> = None;
    let e = none.ok_or_api_code(Code::Silent).expect_err("none");

    assert_eq!(e.code().to_string(), "user.silent");
    assert!(e.message().is_none());
}

#[test]
fn ok_or_api_code_interpolates_variant_fields() {
    let none: Option<u32> = None;
    let e = none.ok_or_api_code(Code::MissingOrder { id: 7 }).expect_err("none");

    assert_eq!(e.code().to_string(), "order.missing");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("order 7 is missing"));
}

#[test]
fn ok_or_api_code_passes_the_value_through_on_some() {
    let some: Option<i32> = Some(5);
    assert_eq!(some.ok_or_api_code(Code::NotFound).expect("some"), 5);
}

#[test]
fn bool_ok_or_api_code_guards_with_the_declared_message() {
    assert!(true.ok_or_api_code(Code::NotFound).is_ok());

    let e = false.ok_or_api_code(Code::NotFound).expect_err("false");
    assert_eq!(e.code().to_string(), "user.not_found");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("user was not found"));
}

/// Absence is not a wrapped failure — there is no underlying error to keep, so
/// the `Option`/`bool` constructors must leave the source chain empty.
#[test]
fn ok_or_api_code_sets_no_source() {
    let none: Option<u32> = None;
    assert!(
        none.ok_or_api_code(Code::NotFound)
            .expect_err("none")
            .source()
            .is_none()
    );
    assert!(
        false
            .ok_or_api_code(Code::NotFound)
            .expect_err("false")
            .source()
            .is_none()
    );
}

/// `#[track_caller]` must survive the extra `From` hop, otherwise every error
/// raised this way points at `ok_or_error.rs` instead of the caller.
#[test]
fn ok_or_api_code_reports_the_caller_location() {
    let none: Option<u32> = None;
    let expected_line = line!() + 1;
    let e = none.ok_or_api_code(Code::NotFound).expect_err("none");

    let location = ApiErrorHandler::location(&e);
    assert_eq!(location.line(), expected_line);
    assert!(
        location.file().ends_with("test_ok_or_error.rs"),
        "location pointed at {}, not the call site",
        location.file(),
    );
}

/// Everything the `From` impl configures — not just the message — has to reach
/// the caller, since `ok_or_api_code` returns that error rather than rebuilding it.
#[test]
fn ok_or_api_code_keeps_the_full_error_the_from_impl_built() {
    #[derive(Clone, Debug)]
    enum Rich {
        Denied,
    }
    impl std::fmt::Display for Rich {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "denied")
        }
    }
    impl From<Rich> for ApiError<Rich> {
        fn from(code: Rich) -> Self {
            error(code)
                .with_message("access denied")
                .with_status(403)
                .with_meta(serde_json::json!({ "scope": "admin" }))
        }
    }

    let e = false.ok_or_api_code(Rich::Denied).expect_err("false");

    assert_eq!(e.status(), 403);
    assert!(e.has_status());
    assert_eq!(e.message().map(|m| m.as_ref()), Some("access denied"));
    assert_eq!(e.meta(), Some(&serde_json::json!({ "scope": "admin" })));
}
