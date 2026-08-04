use crate::{ApiError, ApiErrorCode, ApiErrorHandler, NoData};
use poem::http::StatusCode;
use poem::web::Json;
use poem::{IntoResponse, Response};
use std::sync::Arc;

impl<C: ApiErrorCode> poem::error::ResponseError for ApiError<C> {
    fn status(&self) -> StatusCode {
        // Reads the transport status set via `with_status`/`with_code_status`
        // (default 200), matching the actix and axum adapters. A bogus value
        // falls back to the default rather than a 5xx — see `resolve_status`.
        StatusCode::from_u16(crate::resolve_status(ApiError::status(self))).unwrap_or(StatusCode::OK)
    }

    fn as_response(&self) -> Response {
        // Same builder as the other adapters, so every framework emits identical JSON.
        // `as_response` owns the whole response, so apply the status here too
        // (poem does not merge `status()` into a custom `as_response`).
        let mut response = Json(self.into_api_response::<NoData>()).into_response();
        response.set_status(poem::error::ResponseError::status(self));
        #[cfg(feature = "rpc-status-header")]
        response.headers_mut().insert(
            poem::http::HeaderName::from_static(crate::rpc_status::X_RPC_STATUS),
            poem::http::HeaderValue::from_static(crate::rpc_status::ERROR),
        );
        // Stash the type-erased error for middleware, matching the axum and
        // actix adapters. `as_response` only has `&self`, so this clones — see
        // the note in the actix adapter.
        response
            .extensions_mut()
            .insert(Arc::new(self.clone()) as Arc<dyn ApiErrorHandler>);
        response
    }
}

/// Pull the type-erased error back out of a poem response's extensions.
///
/// The poem counterpart of
/// [`response_get_api_error`](crate::response_get_api_error).
pub fn response_get_api_error_poem(response: &Response) -> Option<&Arc<dyn ApiErrorHandler>> {
    response.extensions().get::<Arc<dyn ApiErrorHandler>>()
}
