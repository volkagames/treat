//! Coverage for the three derive macros: `ApiError`, `ApiErrorCode` and
//! `FromErrorMessage`.
#![cfg(feature = "derive")]

use treat::prelude::*;
use treat::{ApiError, ErrorMessage};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// #[derive(ApiError)] — maps a thiserror enum onto ApiError<&'static str>
// ---------------------------------------------------------------------------

#[derive(Error, Debug, ApiError)]
enum ChatError {
    #[error("Access denied")]
    #[code("access_denied")]
    AccessDenied,

    #[catch_all]
    #[error("internal error")]
    #[code("bad_request")]
    Internal(#[source] erris::Report),
}

#[test]
fn api_error_derive_maps_code_and_display_message() {
    let e: ApiError = ChatError::AccessDenied.into();
    assert_eq!(*e.code(), "access_denied");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("Access denied"));
}

#[test]
fn api_error_derive_catch_all_from_report() {
    let e: ChatError = erris::report!("kaboom").into();
    assert!(matches!(e, ChatError::Internal(_)));

    let api: ApiError = e.into();
    assert_eq!(*api.code(), "bad_request");
    assert_eq!(api.message().map(|m| m.as_ref()), Some("internal error"));
}

// ---------------------------------------------------------------------------
// #[derive(ApiErrorCode)] — a typed error-code enum
// ---------------------------------------------------------------------------

type PortalError = ApiError<PortalErrorCode>;

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, ApiErrorCode)]
enum PortalErrorCode {
    account_already_exist,
    #[code("other_code")]
    #[message("other_message")]
    account_info_invalid,
    #[message("account with id {account_id} not found")]
    account_not_found {
        account_id: u64,
    },
    #[message("account foo {arg_0} {arg_2}")] // arg_1 intentionally skipped
    account_abc(i64, String, u32),
}

#[test]
fn api_error_code_display_defaults_to_variant_name() {
    let e: PortalError = PortalErrorCode::account_already_exist.into();
    assert_eq!(*e.code(), PortalErrorCode::account_already_exist);
    assert_eq!(e.code().to_string(), "account_already_exist");
}

#[test]
fn api_error_code_code_and_message_attributes() {
    let e: PortalError = PortalErrorCode::account_info_invalid.into();
    assert_eq!(e.code().to_string(), "other_code");
    assert_eq!(e.to_error_message().code, "other_code");
    assert_eq!(e.to_error_message().message.as_deref(), Some("other_message"));
}

#[test]
fn api_error_code_interpolates_named_and_tuple_fields() {
    let e: PortalError = PortalErrorCode::account_not_found { account_id: 42 }.into();
    assert_eq!(
        e.to_error_message().message.as_deref(),
        Some("account with id 42 not found")
    );

    let e: PortalError = PortalErrorCode::account_abc(1, "x".to_string(), 5).into();
    assert_eq!(e.to_error_message().message.as_deref(), Some("account foo 1 5"));
}

#[test]
fn api_error_code_into_result() {
    let r: Result<ApiResponse<()>, PortalErrorCode> = PortalErrorCode::account_already_exist.into();
    assert!(r.is_err());
}

// ---------------------------------------------------------------------------
// #[derive(FromErrorMessage)] — ErrorMessage / ApiError -> typed enum
// ---------------------------------------------------------------------------

#[derive(Error, FromErrorMessage, Debug, PartialEq)]
enum ClientError {
    #[error("not found")]
    #[code("not_found")]
    NotFound(ErrorMessage),

    #[error("other")]
    #[code("_")]
    Other(ErrorMessage),
}

#[test]
fn from_error_message_maps_by_code_with_catch_all() {
    let known: ClientError = ErrorMessage {
        code: "not_found".to_string(),
        message: None,
        meta: None,
        ..Default::default()
    }
    .into();
    assert!(matches!(known, ClientError::NotFound(_)));

    let unknown: ClientError = ErrorMessage {
        code: "whatever".to_string(),
        message: None,
        meta: None,
        ..Default::default()
    }
    .into();
    assert!(matches!(unknown, ClientError::Other(_)));
}

#[test]
fn from_error_message_from_api_error_by_ref_and_value() {
    let mapped: ClientError = (&treat::error("not_found")).into();
    assert!(matches!(mapped, ClientError::NotFound(_)));

    let mapped: ClientError = treat::error("not_found").into();
    assert!(matches!(mapped, ClientError::NotFound(_)));
}

// Regression (F2): the `#[code("_")]` catch-all is emitted LAST regardless of
// declaration order. Here it is declared FIRST; a concrete code must still route
// to its own variant instead of being shadowed into the catch-all.
#[derive(FromErrorMessage, Debug, PartialEq)]
enum CatchAllFirst {
    #[code("_")]
    Fallback(ErrorMessage),
    #[code("known")]
    Known(ErrorMessage),
}

#[test]
fn from_error_message_catch_all_first_does_not_shadow_concrete() {
    let known: CatchAllFirst = ErrorMessage {
        code: "known".to_string(),
        ..Default::default()
    }
    .into();
    assert!(matches!(known, CatchAllFirst::Known(_)));

    let unknown: CatchAllFirst = ErrorMessage {
        code: "whatever".to_string(),
        ..Default::default()
    }
    .into();
    assert!(matches!(unknown, CatchAllFirst::Fallback(_)));
}

// Regression (F3): struct (named-field) variants are constructed from their real
// field ident (here `source`, not a hardcoded `err`) with no illegal functional
// record update. Previously this failed to compile with E0436.
#[allow(dead_code)]
#[derive(FromErrorMessage, Debug)]
enum NamedClientError {
    #[code("not_found")]
    NotFound { source: ErrorMessage },
    #[code("_")]
    Other { source: ErrorMessage },
}

#[test]
fn from_error_message_supports_named_field_variants() {
    let found: NamedClientError = ErrorMessage {
        code: "not_found".to_string(),
        ..Default::default()
    }
    .into();
    assert!(matches!(found, NamedClientError::NotFound { .. }));

    let other: NamedClientError = ErrorMessage {
        code: "zzz".to_string(),
        ..Default::default()
    }
    .into();
    assert!(matches!(other, NamedClientError::Other { .. }));
}

// Regression (F4): a `#[code("...")]` string containing `{`/`}` must be printed
// literally, not treated as a `write!` format string (previously E0425).
#[allow(dead_code)]
#[derive(Clone, Debug, ApiErrorCode)]
enum BraceCode {
    #[code("missing {field} value")]
    Weird,
}

#[test]
fn api_error_code_display_allows_braces_in_code() {
    assert_eq!(BraceCode::Weird.to_string(), "missing {field} value");
}
