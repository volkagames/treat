//! G2: validation → locators. Covers the `serde-path` deserializer (pointer on
//! parse failure), the `validator` bridge (field messages + pointers), and the
//! `Validated<T>` typestate.
#![cfg(all(feature = "serde-path", feature = "validator"))]

use treat::{ValidateApi, Validated, deserialize_body, validation_error_messages};
use serde::Deserialize;
use validator::Validate;

#[derive(Debug, Deserialize)]
struct Body {
    #[allow(dead_code)]
    email: String,
    age: u8,
}

#[test]
fn deserialize_body_reports_pointer_on_type_mismatch() {
    let err = deserialize_body::<Body>(br#"{"email": "a@b.com", "age": "old"}"#).expect_err("bad body");
    assert_eq!(*err.code(), "invalid_body");
    assert_eq!(err.error_source().and_then(|s| s.pointer.as_deref()), Some("/age"));
    assert!(err.message().is_some());
}

#[test]
fn deserialize_body_omits_pointer_on_top_level_failure() {
    // Not an object at all -> serde fails before entering any field.
    let err = deserialize_body::<Body>(br#"[]"#).expect_err("bad body");
    assert_eq!(*err.code(), "invalid_body");
    assert!(err.error_source().is_none());
}

#[test]
fn deserialize_body_succeeds_on_valid_input() {
    let body = deserialize_body::<Body>(br#"{"email": "a@b.com", "age": 30}"#).expect("valid");
    assert_eq!(body.age, 30);
}

#[derive(Debug, Deserialize, Validate)]
struct Signup {
    #[validate(email)]
    email: String,
    #[validate(range(min = 18))]
    age: u8,
    #[validate(nested)]
    address: Address,
}

#[derive(Debug, Deserialize, Validate)]
struct Address {
    #[validate(length(min = 1))]
    zip: String,
}

#[test]
fn validation_messages_carry_codes_and_pointers() {
    let bad = Signup {
        email: "nope".into(),
        age: 30,
        address: Address { zip: String::new() },
    };
    let errors = bad.validate().expect_err("invalid");
    let msgs = validation_error_messages(&errors);

    // One entry for `email` (email rule) and one for the nested `address.zip`.
    let email = msgs.iter().find(|m| m.code == "email").expect("email error");
    assert_eq!(email.source.as_ref().and_then(|s| s.pointer.as_deref()), Some("/email"));

    let zip = msgs.iter().find(|m| m.code == "length").expect("length error");
    assert_eq!(
        zip.source.as_ref().and_then(|s| s.pointer.as_deref()),
        Some("/address/zip")
    );
}

#[test]
fn validate_api_builds_a_failure_envelope() {
    let bad = Signup {
        email: "nope".into(),
        age: 10,
        address: Address { zip: "12345".into() },
    };
    let resp = bad.validate_api().expect_err("invalid");
    // Two failures: bad email and out-of-range age.
    assert_eq!(resp.errors.len(), 2);
    assert!(resp.data.is_none());
}

#[test]
fn validated_typestate_proves_validation() {
    let ok = Signup {
        email: "a@b.com".into(),
        age: 21,
        address: Address { zip: "12345".into() },
    };
    let validated = Validated::new(ok).expect("valid");
    // The wrapped value is reachable but cannot be re-validated (no method exists).
    assert_eq!(validated.get().age, 21);
    assert_eq!(validated.email, "a@b.com"); // via Deref

    let bad = Signup {
        email: "nope".into(),
        age: 21,
        address: Address { zip: "12345".into() },
    };
    assert!(Validated::new(bad).is_err());
}
