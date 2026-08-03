//! Coverage for `WrapApiError` (on `Result`) and `WithErrorCode` (on `Report`).

use leto::{WithErrorCode, WrapApiError};

#[test]
fn wrap_api_error_maps_the_error() {
    let err: erris::Result<i32> = Err(erris::report!("boom"));
    let e = err.wrap_api_error("code").expect_err("err");
    assert_eq!(*e.code(), "code");
    assert!(e.source().is_some());

    let ok: erris::Result<i32> = Ok(1);
    assert_eq!(ok.wrap_api_error("code").expect("ok"), 1);
}

#[test]
fn wrap_api_error_and_message_adds_context() {
    let err: erris::Result<i32> = Err(erris::report!("boom"));
    let e = err.wrap_api_error_and_message("code", "context").expect_err("err");
    assert_eq!(*e.code(), "code");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("context"));
    assert!(e.source().is_some());
}

#[test]
fn wrap_api_error_with_is_lazy() {
    let err: erris::Result<i32> = Err(erris::report!("boom"));
    let e = err.wrap_api_error_with(|| ("code", "context")).expect_err("err");
    assert_eq!(*e.code(), "code");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("context"));

    // the closure must not run on the Ok path
    let ran = std::cell::Cell::new(false);
    let ok: erris::Result<i32> = Ok(1);
    let value = ok
        .wrap_api_error_with(|| {
            ran.set(true);
            ("code", "context")
        })
        .expect("ok");
    assert_eq!(value, 1);
    assert!(!ran.get(), "closure ran on the Ok path");
}

#[test]
fn with_error_code_lifts_a_report() {
    let e = erris::report!("boom").with_error_code("code");
    assert_eq!(*e.code(), "code");
    assert!(e.source().is_some());

    let e = erris::report!("boom").with_error_code_and_message("code", "context");
    assert_eq!(*e.code(), "code");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("context"));
}
