//! Regressions for the core error and response types.

use treat::prelude::*;
use treat::{ErrorMessage, ErrorResponse};

/// `{:#?}` lists every field `{:?}` and the accessors expose, `meta` and
/// `hops` included.
#[test]
fn alternate_debug_shows_meta_and_hops() {
    let err = treat::error("boom")
        .with_meta(serde_json::json!({ "attempt": 3 }))
        .track();
    let rendered = format!("{err:#?}");
    assert!(rendered.contains("meta:"), "{rendered}");
    assert!(rendered.contains("\"attempt\""), "{rendered}");
    assert!(rendered.contains("hops:"), "{rendered}");
    assert!(rendered.contains(file!()), "the hop is this file: {rendered}");
}

/// `inner` / `into_inner` need only the `From` impl `FromErrorMessage`
/// generates: a client enum that is not an `std::error::Error` works.
#[test]
fn inner_needs_only_from_error_message() {
    #[derive(Debug)]
    enum ClientError {
        NotFound,
        Other(#[allow(dead_code)] String),
    }
    impl From<&ErrorMessage> for ClientError {
        fn from(err: &ErrorMessage) -> Self {
            match err.code.as_str() {
                "not_found" => ClientError::NotFound,
                other => ClientError::Other(other.to_string()),
            }
        }
    }
    impl From<ErrorMessage> for ClientError {
        fn from(err: ErrorMessage) -> Self {
            Self::from(&err)
        }
    }

    let resp: ApiResponse<u8> = serde_json::from_str(r#"{"errors":[{"code":"not_found"}]}"#).expect("json");
    assert!(matches!(resp.inner::<ClientError>(), Err(ClientError::NotFound)));
    assert!(matches!(resp.into_inner::<ClientError>(), Err(ClientError::NotFound)));

    let ok: ApiResponse<u8> = success(7);
    assert_eq!(ok.inner::<ClientError>().expect("data").copied(), Some(7));
}

/// An empty `ErrorResponse` is not hidden the way `ApiResponse` hides an empty
/// `errors`; the docs say so.
#[test]
fn an_empty_error_response_serializes_its_empty_list() {
    let json = serde_json::to_string(&ErrorResponse::default()).expect("json");
    assert_eq!(json, r#"{"errors":[]}"#);
}

/// `validator` keeps fields in a `HashMap`; the entries come out ordered by
/// field name at every level whatever the map's iteration order.
#[cfg(feature = "validator")]
#[test]
fn validation_errors_are_ordered_by_field() {
    use validator::Validate;

    #[derive(Validate)]
    struct Inner {
        #[validate(length(min = 5))]
        zeta: String,
        #[validate(length(min = 5))]
        alpha: String,
    }

    #[derive(Validate)]
    struct Outer {
        #[validate(length(min = 5))]
        mike: String,
        #[validate(length(min = 5))]
        bravo: String,
        #[validate(nested)]
        nested: Inner,
        #[validate(length(min = 5))]
        yankee: String,
        #[validate(length(min = 5))]
        delta: String,
        #[validate(length(min = 5))]
        kilo: String,
    }

    let expected = [
        "/bravo",
        "/delta",
        "/kilo",
        "/mike",
        "/nested/alpha",
        "/nested/zeta",
        "/yankee",
    ];
    // Each `validate()` builds a fresh `HashMap` with a fresh random seed, so
    // repeating it exercises many iteration orders.
    for _ in 0..32 {
        let value = Outer {
            mike: String::new(),
            bravo: String::new(),
            nested: Inner {
                zeta: String::new(),
                alpha: String::new(),
            },
            yankee: String::new(),
            delta: String::new(),
            kilo: String::new(),
        };
        let errors = value.validate().expect_err("invalid");
        let messages = treat::validation_error_messages(&errors);
        let pointers: Vec<&str> = messages
            .iter()
            .filter_map(|m| m.source.as_ref().and_then(|s| s.pointer.as_deref()))
            .collect();
        assert_eq!(pointers, expected);
    }
}
