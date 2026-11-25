//! `Narrow` on erris's `TrackedResult` (the `tracked` feature, nightly): a
//! service's own functions return `erris::Result<T, Failure<R>>`, and the
//! handler narrows it without going through `into_std()` first.
//!
//! Run with `cargo +nightly nextest run -p treat --features tracked,rpc --test test_tracked_narrow`.
#![cfg(all(feature = "tracked", feature = "derive"))]
#![allow(non_camel_case_types)]

use erris::prelude::*;
use treat::rpc::{Failure, Narrow, UNDECLARED_CODE_META};
use treat::{ApiError, ApiErrorHandler, WrapApiError};

#[derive(Debug, Clone, Copy, PartialEq, treat::ApiErrorCode, treat::rpc::WireEnum)]
enum ErrorCode {
    #[message("internal error")]
    internal_error,
    #[message("not found")]
    not_found,
    #[code("things::gone")]
    things__gone,
}

#[derive(Debug, Clone, Copy, PartialEq, treat::ApiErrorCode, treat::rpc::WireEnum)]
enum GetCodes {
    #[code("things::gone")]
    ThingsGone,
    #[code("internal_error")]
    #[undeclared]
    InternalError,
}

type ServiceResult<T> = erris::Result<T, Failure<ErrorCode>>;

fn hop_lines<C: treat::ApiErrorCode>(e: &ApiError<C>) -> Vec<u32> {
    e.hops().iter().map(|hop| hop.line()).collect()
}

const RAISE: u32 = line!() + 3;
/// A failure with a cause (the `Report` it wraps) and a `meta` entry.
fn raise(code: ErrorCode) -> ServiceResult<u32> {
    let n = erris::tracked::Err(erris::report!("db: no row")).wrap_api_error(code)?;
    Ok(n)
}

const HOP: u32 = line!() + 3;
/// One more function between the raise and the handler: its `?` is a hop.
fn lookup(code: ErrorCode) -> ServiceResult<u32> {
    let n = raise(code).map_err(|e| e.with_meta(serde_json::json!({ "id": 7 })))?;
    Ok(n)
}

#[test]
fn ok_passes_through() {
    let ok: ServiceResult<u32> = Ok(1);
    let narrowed: Result<u32, ApiError<GetCodes>> = ok.narrow();
    assert_eq!(narrowed.expect("ok"), 1);
}

#[test]
fn declared_code_becomes_the_sets_variant() {
    let err = lookup(ErrorCode::things__gone).narrow::<GetCodes>().expect_err("gone");
    assert_eq!(*err.code(), GetCodes::ThingsGone);
    assert_eq!(err.to_error_message().code, "things::gone");
    assert!(err.meta().expect("meta").get(UNDECLARED_CODE_META).is_none());
}

#[test]
fn undeclared_code_becomes_the_undeclared_value() {
    let err = lookup(ErrorCode::not_found)
        .narrow::<GetCodes>()
        .expect_err("not found");
    assert_eq!(*err.code(), GetCodes::InternalError);
    let meta = err.meta().expect("meta");
    assert_eq!(meta[UNDECLARED_CODE_META], "not_found");
    assert_eq!(meta["id"], 7);
}

/// The cause, the raise location and the hops of the tracked error survive,
/// exactly as through `.into_std().narrow()`.
#[test]
fn cause_and_hops_survive_like_into_std() {
    let direct = lookup(ErrorCode::not_found).narrow::<GetCodes>().expect_err("err");
    let via_std = lookup(ErrorCode::not_found)
        .into_std()
        .narrow::<GetCodes>()
        .expect_err("err");

    assert_eq!(direct.source().map(ToString::to_string).as_deref(), Some("db: no row"));
    assert_eq!(ApiErrorHandler::location(&direct).line(), RAISE);
    assert_eq!(hop_lines(&direct), [HOP]);

    assert_eq!(direct.code(), via_std.code());
    assert_eq!(direct.message(), via_std.message());
    assert_eq!(direct.meta(), via_std.meta());
    assert_eq!(direct.status(), via_std.status());
    assert_eq!(
        direct.source().map(ToString::to_string),
        via_std.source().map(ToString::to_string)
    );
    assert_eq!(ApiErrorHandler::location(&direct), ApiErrorHandler::location(&via_std));
    assert_eq!(hop_lines(&direct), hop_lines(&via_std));
}

/// A handler in the shape the RPC generator emits: the trait method returns a
/// std `Result`, the service's own function an `erris::Result`.
#[test]
fn handler_ends_in_narrow() {
    fn handler() -> std::result::Result<u32, ApiError<GetCodes>> {
        lookup(ErrorCode::things__gone).narrow()
    }
    assert_eq!(*handler().expect_err("gone").code(), GetCodes::ThingsGone);
}
