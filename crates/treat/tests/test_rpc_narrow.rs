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

/// A code the set does not declare is reported, in every build, as the set's
/// `#[undeclared]` value with the raised code merged into `meta`: never a
/// panic, which would drop the request in a debug build.
#[test]
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

/// Narrowing an undeclared code does not panic, in a debug build either.
#[test]
fn undeclared_code_does_not_panic() {
    let caught = std::panic::catch_unwind(|| lookup(false, false).narrow::<GetCodes>());
    let err = caught.expect("no panic").expect_err("not found");
    assert_eq!(*err.code(), GetCodes::InternalError);
}
