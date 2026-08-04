//! axum request extractors that report body errors in the `treat` envelope.
//!
//! - [`ApiJson<T>`] parses a JSON body with [`deserialize_body`], so a malformed
//!   or type-mismatched body yields an `ApiError` (`code = "invalid_body"`) with
//!   a JSON Pointer to the offending field, instead of axum's raw `400`.
//! - `ApiValidated<T>` (feature `validator-extract`) additionally runs
//!   `validator::Validate` and yields a `Validated<T>`, failing with one
//!   `errors[]` entry per invalid field.

use crate::deserialize_body;
use axum::body::Bytes;
use axum::extract::FromRequest;
use axum::response::IntoResponse;

/// Extractor: parse a JSON request body into `T`, reporting parse failures as a
/// `treat` error envelope with a field locator. Access the value via `.0`.
#[derive(Debug, Clone, Copy)]
pub struct ApiJson<T>(pub T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = axum::response::Response;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(IntoResponse::into_response)?;
        match deserialize_body::<T>(&bytes) {
            Ok(value) => Ok(ApiJson(value)),
            Err(err) => Err(err.into_response()),
        }
    }
}

/// Extractor: parse a JSON body into `T` and validate it, yielding a
/// [`Validated<T>`](crate::Validated). Parse failures report a field locator;
/// validation failures report one `errors[]` entry per invalid field.
#[cfg(feature = "validator-extract")]
#[derive(Debug, Clone, Copy)]
pub struct ApiValidated<T>(pub crate::Validated<T>);

#[cfg(feature = "validator-extract")]
impl<T, S> FromRequest<S> for ApiValidated<T>
where
    T: serde::de::DeserializeOwned + validator::Validate,
    S: Send + Sync,
{
    type Rejection = axum::response::Response;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(IntoResponse::into_response)?;
        let value = deserialize_body::<T>(&bytes).map_err(IntoResponse::into_response)?;
        // `ApiResponse`'s `IntoResponse` is status-agnostic (a success envelope
        // must stay 200), so a validation rejection sets `REJECTION_STATUS` here.
        let validated = crate::Validated::new(value).map_err(|response| {
            let status = axum::http::StatusCode::from_u16(crate::REJECTION_STATUS)
                .unwrap_or(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
            (status, response).into_response()
        })?;
        Ok(ApiValidated(validated))
    }
}
