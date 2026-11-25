//! What a step of a handler fails with.

use super::http_status;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use treat_core::{ApiError, ApiErrorCode};

/// A response refusing the request: the error of every step a handler takes
/// and of the context checks a project implements.
///
/// The `Response` it is sent as, boxed. A `Response` is over a hundred bytes,
/// and every `Result` a handler threads through `?` would carry that on its
/// success path too (clippy's `result_large_err`, which generated code is
/// checked with); the box is paid once per refused request instead.
///
/// Built from a `Response` or an [`ApiError`] with `From`, so a project's
/// check answers `Err(error(ErrorCode::Forbidden).into())`, and a step that
/// produced a `Response` joins a handler's `?` unchanged.
#[derive(Debug)]
pub struct Refusal(Box<Response>);

impl Refusal {
    /// The refusal with `status` unless it carries one outside `2xx`; see
    /// [`http_status`].
    #[must_use]
    pub fn or_status(self, status: StatusCode) -> Self {
        Self(Box::new(http_status(*self.0, status)))
    }

    /// The status the refusal is sent with.
    pub fn status(&self) -> StatusCode {
        self.0.status()
    }
}

impl From<Response> for Refusal {
    fn from(response: Response) -> Self {
        Self(Box::new(response))
    }
}

impl<C: ApiErrorCode> From<ApiError<C>> for Refusal {
    fn from(error: ApiError<C>) -> Self {
        Self::from(error.into_response())
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        *self.0
    }
}
