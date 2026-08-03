//! actix-web request extractors that report body errors in the `leto`
//! envelope. Mirrors the axum extractors (see [`crate::extract_axum`]).
//!
//! - `ApiJson<T>` parses a JSON body with [`deserialize_body`]; a malformed
//!   body yields an `ApiError` (`code = "invalid_body"`) with a JSON Pointer.
//! - `ApiValidated<T>` (feature `validator-extract`) also runs
//!   `validator::Validate`, yielding a `Validated<T>`.

use crate::deserialize_body;
use actix_web::dev::Payload;
use actix_web::{FromRequest, HttpRequest, web};
use std::future::Future;
use std::pin::Pin;

/// Extractor: parse a JSON request body into `T`, reporting parse failures as a
/// `leto` error envelope with a field locator. Access the value via `.0`.
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
            let bytes = bytes.await.map_err(|e| {
                crate::error("invalid_body")
                    .with_message(e.to_string())
                    .with_status(crate::REJECTION_STATUS)
            })?;
            deserialize_body::<T>(&bytes).map(ApiJson)
        })
    }
}

/// Extractor: parse a JSON body into `T` and validate it, yielding a
/// [`Validated<T>`](crate::Validated). Validation failures report one
/// `errors[]` entry per invalid field.
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
            let bytes = bytes
                .await
                .map_err(|e| {
                    crate::error("invalid_body")
                        .with_message(e.to_string())
                        .into_api_response()
                })
                .map_err(ApiResponseError::new)?;
            let value = deserialize_body::<T>(&bytes).map_err(|e| ApiResponseError::new(e.into_api_response()))?;
            let validated = crate::Validated::new(value).map_err(ApiResponseError::new)?;
            Ok(ApiValidated(validated))
        })
    }
}

/// actix's extractor `Error` is a single value; this newtype lets a multi-error
/// [`ApiResponse`](crate::ApiResponse) (e.g. every invalid field) be returned as
/// a `ResponseError` that renders the full envelope.
///
/// Responds with [`REJECTION_STATUS`](crate::REJECTION_STATUS) (`422`); use
/// [`with_status`](Self::with_status) for a different one.
#[cfg(feature = "validator-extract")]
#[derive(Debug)]
pub struct ApiResponseError(crate::ApiResponse<crate::NoData>, u16);

#[cfg(feature = "validator-extract")]
impl ApiResponseError {
    /// Wrap a failure envelope, responding with [`crate::REJECTION_STATUS`].
    pub fn new(response: crate::ApiResponse<crate::NoData>) -> Self {
        Self(response, crate::REJECTION_STATUS)
    }

    /// Override the HTTP status this rejection responds with.
    pub fn with_status(mut self, status: u16) -> Self {
        self.1 = status;
        self
    }
}

#[cfg(feature = "validator-extract")]
impl std::fmt::Display for ApiResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "request validation failed")
    }
}

#[cfg(feature = "validator-extract")]
impl actix_web::ResponseError for ApiResponseError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        actix_web::http::StatusCode::from_u16(crate::resolve_status(self.1)).unwrap_or(actix_web::http::StatusCode::OK)
    }

    fn error_response(&self) -> actix_web::HttpResponse {
        actix_web::HttpResponse::build(self.status_code()).json(&self.0)
    }
}
