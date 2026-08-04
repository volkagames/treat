#![cfg_attr(feature = "nightly-provide", feature(error_generic_member_access))]
use erris::prelude::*;
use treat::WrapApiError;

#[test]
fn test_api_error_clone() {
    use treat::WithErrorCode;

    let err = erris::report!("some error")
        .with_err(treat::error("foo_err"))
        .with_message("message error")
        .with_err(treat::error("bar_err"))
        .with_err(treat::error("baz_err"))
        .with_error_code("err_code");

    let err2 = err.clone();

    assert_eq!(err.to_string(), err2.to_string());
}

#[test]
fn test_api_error_chain_len() {
    let err = erris::Result::Err(erris::report!("root error"))
        .wrap_report("just report")
        .wrap_api_error_and_message("api error", "and message")
        .wrap_report_with(|| treat::error("api error in callback"))
        .wrap_api_error("just api error")
        .map_err(|e| e.with_verbose());

    let resp: treat::ApiResponse = err.into();
    println!("resp {resp:?}");
    assert_eq!(resp.errors.len(), 3);
}

#[test]
fn test_api_error_chain() {
    use erris::prelude::*;
    use treat::WrapApiError;

    {
        let err = erris::report!(treat::error("foo_err"));
        // full cause chain is opt-in via verbose mode
        let resp: treat::ApiResponse = Err(err).wrap_api_error("err_code").map_err(|e| e.with_verbose()).into();

        println!("resp {resp:?}");
        assert_eq!(resp.errors.len(), 2);
        assert!(resp.has_error_code("foo_err").is_some());
    }

    {
        let err = erris::report!(treat::error("foo_err"));
        let wrap_err: &treat::ApiError = err.unwrap_ref().expect("Failed to downcast error to ApiError");

        let resp: treat::ApiResponse = wrap_err.into();
        println!("resp {resp:?}");
        assert_eq!(resp.errors.len(), 1);
        assert!(resp.has_error_code("foo_err").is_some());
    }

    #[cfg(feature = "nightly-provide")]
    {
        let err = treat::error("foo_err");
        let loc: &treat::Location = std::error::request_value(&err).expect("Failed to get location from error");
        println!("location {loc:?}");
    }

    {
        fn a() -> erris::Result<()> {
            Ok(b().wrap_api_error("a_err")?)
        }

        fn b() -> erris::Result<()> {
            c().wrap_report_with(|| treat::error("b_err"))
        }

        fn c() -> erris::Result<()> {
            d().wrap_report_with(|| erris::report!("c_err"))
        }

        fn d() -> Result<(), treat::ApiError> {
            e().map_err(|err| treat::error("d_err").with_source(err))
        }

        fn e() -> erris::Result<()> {
            Err(treat::error("error_code").with_message("z_message").into())
        }

        let err = a().wrap_api_error("api error").map_err(|e| e.with_verbose());
        println!("err: {err:?}");
        let resp: treat::ApiResponse = err.into();
        println!("res: {resp:?}");
        assert_eq!(resp.errors.len(), 5);
        assert!(resp.has_error_code("b_err").is_some());
    }
}

// A response carries only the top-level error by default; the full cause chain
// is opt-in via verbose mode. This is the single builder shared by every
// framework adapter, so actix and axum serialize identically.
// (The `verbose-error` feature forces verbose globally, so this default-mode
// assertion only holds when it is off.)
#[cfg(not(feature = "verbose-error"))]
#[test]
fn test_response_top_only_by_default() {
    let outer = treat::error("outer_err").with_source(erris::report!(treat::error("inner_err")));

    let default = outer.into_api_response::<()>();
    assert_eq!(default.errors.len(), 1);
    assert_eq!(default.errors[0].code, "outer_err");
    assert!(default.has_error_code("inner_err").is_none());

    let verbose = outer.with_verbose().into_api_response::<()>();
    assert_eq!(verbose.errors.len(), 2);
    assert!(verbose.has_error_code("outer_err").is_some());
    assert!(verbose.has_error_code("inner_err").is_some());
}
