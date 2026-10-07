//! Coverage for `ApiError` as the error of an `erris::Result<T, ApiError>`
//! (the `tracked` feature, nightly), and for such a result as an axum handler's
//! return type (`tracked` + `axum`).
//!
//! Run with `cargo +nightly nextest run -p treat --features tracked,axum --test test_tracked_api_error`.
#![cfg(feature = "tracked")]

use erris::prelude::*;
use treat::{ApiError, ApiErrorHandler, ApiResponse, WrapApiError, error, success};

#[derive(Clone, Debug, Default, treat::ApiErrorCode)]
enum Code {
    #[default]
    #[code("internal")]
    #[status(500)]
    Internal,
    #[code("not_found")]
    #[status(404)]
    NotFound,
}

type ApiResult<T> = erris::Result<T, ApiError<Code>>;

/// Lines of the hops `?` recorded on the error, in the order it passed them.
fn hop_lines(e: &ApiError<Code>) -> Vec<u32> {
    e.hops().iter().map(|hop| hop.line()).collect()
}

const RAISE: u32 = line!() + 2;
fn raise() -> ApiResult<u32> {
    Err(error(Code::NotFound).with_status(404))
}

#[test]
fn question_mark_tracks_every_hop() {
    const MID: u32 = line!() + 2;
    fn mid() -> ApiResult<u32> {
        let n = raise()?;
        Ok(n)
    }
    const TOP: u32 = line!() + 2;
    fn top() -> ApiResult<u32> {
        let n = mid()?;
        Ok(n)
    }

    let e = top().unwrap_err();
    assert!(matches!(e.code(), Code::NotFound));
    assert_eq!(ApiErrorHandler::location(&e).line(), RAISE);
    assert_eq!(hop_lines(&e), [MID, TOP]);
    assert!(e.source().is_none(), "a hop is not a cause");
}

#[test]
fn a_hop_on_the_raise_line_is_not_recorded() {
    fn raised_inline() -> ApiResult<u32> {
        Err(error(Code::NotFound))?;
        Ok(1)
    }
    assert!(hop_lines(&raised_inline().unwrap_err()).is_empty());

    // `wrap_api_error` hands back a std `Result`; its `?` is on the line the
    // `ApiError` was built on.
    fn wrapped() -> ApiResult<u32> {
        let n = erris::tracked::Err(erris::report!("boom")).wrap_api_error(Code::Internal)?;
        Ok(n)
    }
    let e = wrapped().unwrap_err();
    assert_eq!(e.source().map(ToString::to_string).as_deref(), Some("boom"));
    assert!(hop_lines(&e).is_empty());
}

#[test]
fn std_result_with_an_api_error_is_tracked() {
    fn std_raise() -> Result<u32, ApiError<Code>> {
        std::result::Result::Err(error(Code::NotFound))
    }
    const HOP: u32 = line!() + 2;
    fn hop() -> ApiResult<u32> {
        let n = std_raise()?;
        Ok(n)
    }

    assert_eq!(hop_lines(&hop().unwrap_err()), [HOP]);
}

#[test]
fn ok_responds_with_success() {
    fn handler() -> ApiResult<ApiResponse<u32>> {
        Ok(success(7))
    }
    assert_eq!(handler().expect("ok").inner_data().ok(), Some(&7));
}

#[cfg(feature = "axum")]
mod axum_handler {
    use super::*;
    use axum::body::Body;
    use axum::handler::Handler;
    use axum::http::{Request, StatusCode};

    async fn found() -> ApiResult<ApiResponse<u32>> {
        Ok(success(1))
    }

    async fn missing() -> ApiResult<ApiResponse<u32>> {
        let n = raise()?;
        Ok(success(n))
    }

    async fn call<H, T>(handler: H) -> axum::response::Response
    where
        H: Handler<T, ()>,
    {
        handler.call(Request::new(Body::empty()), ()).await
    }

    #[tokio::test]
    async fn an_axum_handler_returns_a_tracked_result() {
        assert_eq!(call(found).await.status(), StatusCode::OK);

        let response = call(missing).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let handler = treat::response_get_api_error(&response).expect("api error handler");
        assert_eq!(handler.code(), "not_found");
    }
}
