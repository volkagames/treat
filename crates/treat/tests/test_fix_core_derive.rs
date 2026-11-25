//! Regressions for the derive macros' generated code in a user crate.
//!
//! `unused_variables` is denied for the whole file: a `#[message]` that reads
//! only some of a variant's fields used to bind the rest anyway, which warns in
//! the user's crate and breaks a `-D warnings` build. This file failing to
//! compile is the regression.
#![cfg(feature = "derive")]
#![deny(unused_variables)]

use thiserror::Error;
use treat::prelude::*;
use treat::{ApiErrorStatus, ErrorMessage};

// The fields no message reads are never read at all, which `dead_code` reports
// whatever the derive does; that is the test's own business.
#[allow(dead_code)]
#[derive(Clone, Debug, ApiErrorCode)]
enum PartialMessage {
    #[code("named")]
    #[message("only {a}")]
    Named { a: u32, b: u32 },
    #[code("tuple")]
    #[message("only {arg_1:>arg_2$}")]
    Tuple(u32, u32, usize),
    #[code("none")]
    #[message("no fields read")]
    NoneRead { a: u32 },
}

#[test]
fn a_message_reads_only_the_fields_it_names() {
    let err: ApiError<PartialMessage> = PartialMessage::Named { a: 1, b: 2 }.into();
    assert_eq!(err.message().map(|m| m.as_ref()), Some("only 1"));

    // `arg_2$` (the width) is read too, so it is bound alongside `arg_1`.
    let err: ApiError<PartialMessage> = PartialMessage::Tuple(9, 7, 3).into();
    assert_eq!(err.message().map(|m| m.as_ref()), Some("only   7"));

    let err: ApiError<PartialMessage> = PartialMessage::NoneRead { a: 5 }.into();
    assert_eq!(err.message().map(|m| m.as_ref()), Some("no fields read"));
}

/// A generic code enum: its impls carry the parameters and its own bounds.
#[derive(Clone, Debug, ApiErrorCode)]
enum GenericCode<T: Clone + std::fmt::Debug + std::fmt::Display + Send + Sync + 'static> {
    #[code("bad_value")]
    #[message("bad value {value}")]
    #[status(422)]
    BadValue { value: T },
    #[code("other")]
    Other,
}

#[test]
fn a_generic_code_enum_derives() {
    let err: ApiError<GenericCode<u8>> = GenericCode::BadValue { value: 7u8 }.into();
    assert_eq!(err.code().to_string(), "bad_value");
    assert_eq!(err.message().map(|m| m.as_ref()), Some("bad value 7"));
    assert_eq!(err.status(), 422);
    assert_eq!(GenericCode::<u8>::Other.status_code(), treat::DEFAULT_ERROR_STATUS);

    let result: Result<ApiResponse<u8>, GenericCode<String>> = GenericCode::Other.into();
    assert!(result.is_err());
}

/// A generic `thiserror` enum whose `Display` holds only under a bound
/// `thiserror` adds itself.
#[derive(Debug, Error, ApiError)]
enum GenericServiceError<T: std::fmt::Debug> {
    #[error("bad input {0:?}")]
    #[code("bad_input")]
    #[status(400)]
    BadInput(T),

    #[catch_all]
    #[error("internal")]
    #[code("internal")]
    Internal(#[source] treat::erris::Report),
}

#[test]
fn a_generic_api_error_enum_derives() {
    let err: ApiError = GenericServiceError::BadInput(3u8).into();
    assert_eq!(*err.code(), "bad_input");
    assert_eq!(err.message().map(|m| m.as_ref()), Some("bad input 3"));
    assert_eq!(err.status(), 400);

    let caught: GenericServiceError<u8> = treat::erris::report!("db down").into();
    assert!(matches!(caught, GenericServiceError::Internal(_)));
}

/// A tuple variant with several fields is fine when it is not the catch-all.
#[derive(Debug, Error, ApiError)]
enum MultiField {
    #[error("pair {0}/{1}")]
    #[code("pair")]
    Pair(u8, u8),
}

#[test]
fn a_multi_field_tuple_variant_converts() {
    let err: ApiError = MultiField::Pair(1, 2).into();
    assert_eq!(*err.code(), "pair");
    assert_eq!(err.message().map(|m| m.as_ref()), Some("pair 1/2"));
}

/// A generic client enum: the field type is generic, bounded on what the
/// conversion needs.
#[derive(Debug, FromErrorMessage)]
enum GenericClientError<E> {
    #[code("gone")]
    Gone(E),
    #[code("_")]
    Other(E),
}

#[test]
fn a_generic_from_error_message_enum_derives() {
    let message = ErrorMessage {
        code: "gone".into(),
        ..ErrorMessage::default()
    };
    let typed: GenericClientError<ErrorMessage> = (&message).into();
    assert!(matches!(typed, GenericClientError::Gone(_)));

    let typed: GenericClientError<ErrorMessage> = treat::error("unknown").into();
    assert!(matches!(typed, GenericClientError::Other(_)));
}
