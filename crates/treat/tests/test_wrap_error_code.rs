//! Coverage for `WrapApiError` (on `Result`) and `WithErrorCode` (on `Report`).

use treat::{ApiError, ApiErrorHandler, WithErrorCode, WrapApiError, error};

/// Exercises the `From<C> for ApiError<C>` route the `*_api_code` methods take.
#[derive(Clone, Debug, treat::ApiErrorCode)]
enum Code {
    #[code("internal")]
    #[message("the request could not be completed")]
    Internal,

    /// No `#[message]` — the declared-message route has nothing to apply.
    #[code("internal.silent")]
    Silent,
}

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

#[test]
fn wrap_api_code_applies_the_declared_message_and_keeps_the_cause() {
    let err: erris::Result<i32> = Err(erris::report!("boom"));
    let e = err.wrap_api_code(Code::Internal).expect_err("err");

    assert_eq!(e.code().to_string(), "internal");
    assert_eq!(
        e.message().map(|m| m.as_ref()),
        Some("the request could not be completed")
    );
    assert!(e.source().is_some(), "the cause must survive for the logs");
}

/// The documented contrast: `wrap_api_error` builds from the code directly and
/// drops `#[message]`; `wrap_api_code` goes through `From` and keeps it. Both
/// keep the source.
#[test]
fn wrap_api_error_skips_the_declared_message() {
    let err: erris::Result<i32> = Err(erris::report!("boom"));
    let bare = err.wrap_api_error(Code::Internal).expect_err("err");

    let err: erris::Result<i32> = Err(erris::report!("boom"));
    let via_code = err.wrap_api_code(Code::Internal).expect_err("err");

    assert!(bare.message().is_none(), "the direct path must not apply #[message]");
    assert!(via_code.message().is_some());
    assert_eq!(bare.code().to_string(), via_code.code().to_string());
    assert!(bare.source().is_some() && via_code.source().is_some());
}

#[test]
fn wrap_api_code_leaves_a_message_less_code_bare() {
    let err: erris::Result<i32> = Err(erris::report!("boom"));
    let e = err.wrap_api_code(Code::Silent).expect_err("err");

    assert_eq!(e.code().to_string(), "internal.silent");
    assert!(e.message().is_none());
    assert!(e.source().is_some());
}

#[test]
fn wrap_api_code_passes_the_value_through_on_ok() {
    let ok: erris::Result<i32> = Ok(1);
    assert_eq!(ok.wrap_api_code(Code::Internal).expect("ok"), 1);
}

/// Wrapping accepts any `IntoReport` error, not just an `erris::Report`.
#[test]
fn wrap_api_code_accepts_a_foreign_error() {
    let failed: Result<(), std::io::Error> = Err(std::io::Error::other("disk"));
    let e = failed.wrap_api_code(Code::Internal).expect_err("err");

    assert_eq!(e.source().expect("source").to_string(), "disk");
}

/// `#[track_caller]` must survive the extra `From` hop, otherwise every wrapped
/// error points at `wrap_error_code.rs` instead of the caller.
#[test]
fn wrap_api_code_reports_the_caller_location() {
    let err: erris::Result<i32> = Err(erris::report!("boom"));
    let expected_line = line!() + 1;
    let e = err.wrap_api_code(Code::Internal).expect_err("err");

    let location = ApiErrorHandler::location(&e);
    assert_eq!(location.line(), expected_line);
    assert!(
        location.file().ends_with("test_wrap_error_code.rs"),
        "location pointed at {}, not the call site",
        location.file(),
    );
}

/// When the `From` impl already attached a cause, wrapping must *chain* onto it
/// rather than replace it — losing either end would break the trail.
#[test]
fn wrap_api_code_chains_onto_a_source_the_from_impl_set() {
    #[derive(Clone, Debug)]
    enum Rich {
        Boom,
    }
    impl std::fmt::Display for Rich {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "boom")
        }
    }
    impl From<Rich> for ApiError<Rich> {
        fn from(code: Rich) -> Self {
            error(code)
                .with_message("rich message")
                .with_status(404)
                .with_error(erris::report!("preexisting"))
        }
    }

    let failed: Result<(), std::io::Error> = Err(std::io::Error::other("disk"));
    let e = failed.wrap_api_code(Rich::Boom).expect_err("err");

    assert_eq!(e.status(), 404, "the status the From impl set must survive");
    assert_eq!(e.message().map(|m| m.as_ref()), Some("rich message"));

    let chain = format!("{e:?}");
    assert!(chain.contains("disk"), "wrapped cause missing from:\n{chain}");
    assert!(
        chain.contains("preexisting"),
        "cause set by the From impl was dropped:\n{chain}"
    );
}

#[test]
fn with_api_code_applies_the_declared_message() {
    let e = erris::report!("boom").with_api_code(Code::Internal);

    assert_eq!(e.code().to_string(), "internal");
    assert_eq!(
        e.message().map(|m| m.as_ref()),
        Some("the request could not be completed")
    );
    assert!(e.source().is_some());
}

/// `with_api_code` is to `with_error_code` what `wrap_api_code` is to
/// `wrap_api_error`: same code and source, declared message only on the former.
#[test]
fn with_error_code_skips_the_declared_message() {
    let bare = erris::report!("boom").with_error_code(Code::Internal);
    let via_code = erris::report!("boom").with_api_code(Code::Internal);

    assert!(bare.message().is_none(), "the direct path must not apply #[message]");
    assert!(via_code.message().is_some());
    assert_eq!(bare.code().to_string(), via_code.code().to_string());
}

#[test]
fn with_api_code_reports_the_caller_location() {
    let expected_line = line!() + 1;
    let e = erris::report!("boom").with_api_code(Code::Internal);

    let location = ApiErrorHandler::location(&e);
    assert_eq!(location.line(), expected_line);
    assert!(
        location.file().ends_with("test_wrap_error_code.rs"),
        "location pointed at {}, not the call site",
        location.file(),
    );
}
