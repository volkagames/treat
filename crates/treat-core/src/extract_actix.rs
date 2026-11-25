//! actix-web request extractors that report body errors in the `treat`
//! envelope. Mirrors the axum extractors (see [`crate::extract_axum`]).
//!
//! - `ApiJson<T>` parses a JSON body with [`deserialize_body`]; a malformed
//!   body yields an `ApiError` (`code = "invalid_body"`) with a JSON Pointer and
//!   status `422`.
//! - `ApiValidated<T>` (feature `validator-extract`) also runs
//!   `validator::Validate`, yielding a `Validated<T>`; a validation failure is
//!   one `errors[]` entry per violated rule, at `422`.
//!
//! A body that cannot be read at all — over the `PayloadConfig` limit, or a
//! broken stream — is an `invalid_body` envelope too. It keeps the status
//! actix gives it: `413` for a body over the limit, `400` otherwise.

use crate::deserialize_body;
use actix_web::dev::Payload;
use actix_web::{FromRequest, HttpRequest, web};
use std::future::Future;
use std::pin::Pin;

/// The `invalid_body` error for a body actix could not read, with actix's
/// message and status.
fn body_read_error(error: &actix_web::Error) -> crate::ApiError {
    let status = error.as_response_error().status_code().as_u16();
    crate::serde_path::body_read_error(error.to_string(), status)
}

/// Extractor: parse a JSON request body into `T`, reporting parse failures as a
/// `treat` error envelope with a field locator. Access the value via `.0`.
#[derive(Debug, Clone, Copy)]
pub struct ApiJson<T>(pub T);

impl<T> FromRequest for ApiJson<T>
where
    T: serde::de::DeserializeOwned + 'static,
{
    type Error = crate::ApiError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let bytes = web::Bytes::from_request(req, payload);
        Box::pin(async move {
            let bytes = bytes.await.map_err(|e| body_read_error(&e))?;
            deserialize_body::<T>(&bytes).map(ApiJson)
        })
    }
}

/// Extractor: parse a JSON body into `T` and validate it, yielding a
/// [`Validated<T>`](crate::Validated). Validation failures report one
/// `errors[]` entry per violated rule.
#[cfg(feature = "validator-extract")]
#[derive(Debug, Clone, Copy)]
pub struct ApiValidated<T>(pub crate::Validated<T>);

#[cfg(feature = "validator-extract")]
impl<T> FromRequest for ApiValidated<T>
where
    T: serde::de::DeserializeOwned + validator::Validate + 'static,
{
    type Error = ApiResponseError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let bytes = web::Bytes::from_request(req, payload);
        Box::pin(async move {
            let bytes = bytes.await.map_err(|e| ApiResponseError::from(body_read_error(&e)))?;
            let value = deserialize_body::<T>(&bytes).map_err(ApiResponseError::from)?;
            let validated = crate::Validated::new(value).map_err(ApiResponseError::new)?;
            Ok(ApiValidated(validated))
        })
    }
}

/// actix's extractor `Error` is a single value; this lets a multi-error
/// [`ApiResponse`](crate::ApiResponse) (e.g. every invalid field) be returned as
/// a `ResponseError` that renders the full envelope.
///
/// Built from an envelope with [`new`](Self::new), it responds with
/// [`REJECTION_STATUS`](crate::REJECTION_STATUS) (`422`); use
/// [`with_status`](Self::with_status) for a different one. Built from an
/// [`ApiError`](crate::ApiError) (`From`), it renders exactly as that error does
/// — its status, and the error stashed in the response extensions for
/// [`response_get_api_error_actix`](crate::response_get_api_error_actix) and the
/// loggers. Either way the response carries the `X-RPC-Status` header under the
/// `rpc-status-header` feature.
#[cfg(feature = "validator-extract")]
#[derive(Debug)]
pub struct ApiResponseError(Rejection);

#[cfg(feature = "validator-extract")]
#[derive(Debug)]
enum Rejection {
    /// One error (the body could not be read or parsed), kept whole so it
    /// renders through its own `ResponseError`.
    Error(crate::ApiError),
    /// A failure envelope (validation) and the status it is answered with.
    Envelope(crate::ApiResponse<crate::NoData>, u16),
}

#[cfg(feature = "validator-extract")]
impl ApiResponseError {
    /// Wrap a failure envelope, responding with [`crate::REJECTION_STATUS`].
    pub fn new(response: crate::ApiResponse<crate::NoData>) -> Self {
        Self(Rejection::Envelope(response, crate::REJECTION_STATUS))
    }

    /// Override the HTTP status this rejection responds with.
    ///
    /// `status` must be a valid HTTP status code
    /// ([`VALID_STATUS_RANGE`](crate::VALID_STATUS_RANGE)), as for
    /// [`ApiError::with_status`](crate::ApiError::with_status): a debug build
    /// panics otherwise; a release build falls back to `200`.
    #[track_caller]
    pub fn with_status(self, status: u16) -> Self {
        match self.0 {
            Rejection::Error(error) => Self(Rejection::Error(error.with_status(status))),
            Rejection::Envelope(response, _) => {
                debug_assert!(
                    crate::VALID_STATUS_RANGE.contains(&status),
                    "invalid HTTP status {status} passed to `with_status`; must be {}..={}",
                    crate::VALID_STATUS_RANGE.start(),
                    crate::VALID_STATUS_RANGE.end(),
                );
                Self(Rejection::Envelope(response, status))
            }
        }
    }
}

#[cfg(feature = "validator-extract")]
impl From<crate::ApiError> for ApiResponseError {
    fn from(error: crate::ApiError) -> Self {
        Self(Rejection::Error(error))
    }
}

#[cfg(feature = "validator-extract")]
impl std::fmt::Display for ApiResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Rejection::Error(error) => std::fmt::Display::fmt(error, f),
            Rejection::Envelope(..) => write!(f, "request validation failed"),
        }
    }
}

#[cfg(feature = "validator-extract")]
impl actix_web::ResponseError for ApiResponseError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match &self.0 {
            Rejection::Error(error) => actix_web::ResponseError::status_code(error),
            Rejection::Envelope(_, status) => {
                actix_web::http::StatusCode::from_u16(crate::resolve_status(*status))
                    .unwrap_or(actix_web::http::StatusCode::OK)
            }
        }
    }

    fn error_response(&self) -> actix_web::HttpResponse {
        match &self.0 {
            Rejection::Error(error) => actix_web::ResponseError::error_response(error),
            Rejection::Envelope(response, _) => {
                let mut builder = actix_web::HttpResponse::build(self.status_code());
                #[cfg(feature = "rpc-status-header")]
                builder.insert_header((crate::rpc_status::X_RPC_STATUS, response.rpc_status()));
                builder.json(response)
            }
        }
    }
}
