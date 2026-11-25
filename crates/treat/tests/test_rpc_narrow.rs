//! `Narrow::narrow`: a flat registry failure reported through the set of codes
//! an operation declares, matched by wire name.
#![cfg(feature = "derive")]
#![allow(non_camel_case_types)]

use serde::{Deserialize, Serialize};
use treat::prelude::*;
use treat::rpc::{Failure, Narrow, RuntimeCodes, UNDECLARED_CODE_META};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ApiErrorCode, treat::rpc::WireEnum)]
enum ErrorCode {
    #[message("invalid body")]
    invalid_body,
    #[message("internal error")]
    internal_error,
    #[message("not found")]
    not_found,
    #[message("gone")]
    #[code("things::gone")]
    #[serde(rename = "things::gone")]
    things__gone,
}

impl RuntimeCodes for ErrorCode {
    const INVALID_BODY: Self = Self::invalid_body;
    const INTERNAL_ERROR: Self = Self::internal_error;
}

/// The set one operation declares: a subset of the registry under its own
/// variant names, joined to it by wire name only.
#[derive(Debug, Clone, Copy, PartialEq, ApiErrorCode, treat::rpc::WireEnum)]
enum GetCodes {
    #[code("things::gone")]
    ThingsGone,
    #[code("invalid_body")]
    InvalidBody,
    #[code("internal_error")]
    #[undeclared]
    InternalError,
}

#[derive(Debug, Clone, Copy, PartialEq, ApiErrorCode, treat::rpc::WireEnum)]
enum ListCodes {
    #[code("not_found")]
    NotFound,
    #[code("internal_error")]
    #[undeclared]
    InternalError,
}

/// A check shared below the handlers, answering with the flat registry.
fn lookup(found: bool, gone: bool) -> Result<u32, Failure<ErrorCode>> {
    if gone {
        return Err(treat::error(ErrorCode::things__gone));
    }
    if !found {
        return Err(ApiError::from(ErrorCode::not_found).with_meta(serde_json::json!({ "id": 7 })));
    }
    Ok(1)
}

/// A handler: `.narrow()?` with the set inferred from the return type.
fn get(found: bool, gone: bool) -> Result<u32, ApiError<GetCodes>> {
    let value = lookup(found, gone).narrow()?;
    Ok(value)
}

fn list(found: bool) -> Result<u32, ApiError<ListCodes>> {
    let value = lookup(found, false).narrow()?;
    Ok(value)
}

#[test]
fn success_passes_through() {
    assert_eq!(get(true, false).ok(), Some(1));
}

#[test]
fn declared_tagged_code_narrows_by_wire_name() {
    let err = get(true, true).expect_err("gone");
    assert_eq!(*err.code(), GetCodes::ThingsGone);
    // `ApiErrorCode` gives the narrowed code the same wire string.
    assert_eq!(err.to_error_message().code, "things::gone");
}

#[test]
fn declared_code_keeps_message_and_meta() {
    let err = list(false).expect_err("not found");
    assert_eq!(*err.code(), ListCodes::NotFound);
    assert_eq!(err.message().map(|m| m.as_ref()), Some("not found"));
    assert_eq!(err.meta().expect("meta")["id"], 7);
    assert!(err.meta().expect("meta").get(UNDECLARED_CODE_META).is_none());
}

/// A debug build asserts on a code the set does not declare, so the gap in
/// `@throws` shows up in tests; a release build reports it as the set's
/// `#[undeclared]` value with the raised code merged into `meta`.
#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "`not_found` was raised but not declared"))]
fn undeclared_code_becomes_the_undeclared_value() {
    let err = get(false, false).expect_err("not found");
    assert_eq!(*err.code(), GetCodes::InternalError);
    let meta = err.meta().expect("meta");
    assert_eq!(meta[UNDECLARED_CODE_META], "not_found");
    assert_eq!(meta["id"], 7);
}

#[cfg(feature = "rpc")]
#[test]
fn runtime_marker_is_a_zero_sized_alias_target() {
    type Rt = treat::rpc::Runtime<ErrorCode>;
    assert_eq!(std::mem::size_of::<Rt>(), 0);
}

#[test]
fn http_status_survives_narrowing() {
    fn conflict() -> Result<u32, Failure<ErrorCode>> {
        Err(ApiError::from(ErrorCode::things__gone).with_status(409))
    }

    let err = conflict().narrow::<GetCodes>().expect_err("gone");
    assert_eq!(*err.code(), GetCodes::ThingsGone);
    assert_eq!(err.status(), 409);
}

/// The registry needs no `RuntimeCodes` to narrow: narrowing reads only the
/// raised code's wire name.
#[test]
fn registry_without_runtime_codes_narrows() {
    #[derive(Debug, Clone, Copy, PartialEq, ApiErrorCode, treat::rpc::WireEnum)]
    enum Plain {
        #[code("not_found")]
        NotFound,
    }

    let raised: Result<(), Failure<Plain>> = Err(treat::error(Plain::NotFound));
    let err = raised.narrow::<ListCodes>().expect_err("not found");
    assert_eq!(*err.code(), ListCodes::NotFound);
}

std::thread_local! {
    /// Where the last panic on this thread was raised, as the hook below saw it.
    static PANIC_AT: std::cell::Cell<Option<(String, u32)>> = const { std::cell::Cell::new(None) };
}

/// A debug build's assertion on an undeclared code points at the handler's
/// `.narrow()`, not into `treat`: that line is where the missing `@throws`
/// entry belongs.
///
/// The location comes from a panic hook, the one place a panic's location is
/// visible. The hook is process-wide, so it records into a thread-local (each
/// test runs on its own thread) and hands every panic on to the previous hook.
#[test]
fn undeclared_code_panics_at_the_callers_narrow() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let at = info
                .location()
                .map(|location| (location.file().to_string(), location.line()));
            PANIC_AT.set(at);
            previous(info);
        }));
    });

    // The `.narrow()` call is on the line after this one.
    let line = line!() + 1;
    let caught = std::panic::catch_unwind(|| lookup(false, false).narrow::<GetCodes>());
    if cfg!(debug_assertions) {
        assert!(caught.is_err(), "a debug build asserts on an undeclared code");
        assert_eq!(PANIC_AT.take(), Some((file!().to_string(), line)));
    } else {
        let err = caught.expect("no assertion in release").expect_err("not found");
        assert_eq!(*err.code(), GetCodes::InternalError);
        assert_eq!(PANIC_AT.take(), None);
    }
}
