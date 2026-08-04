//! Coverage for the `OkOrError` extension trait on `Option` and `bool`.

use treat::prelude::*;

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
