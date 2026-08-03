use crate::{ApiError, ApiErrorCode, ApiErrorHandler};
use actix_web::body::BoxBody;
use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
use std::sync::Arc;

impl<C: ApiErrorCode> ResponseError for ApiError<C> {
    fn status_code(&self) -> StatusCode {
        // Reads the transport status set via `with_status`/`with_code_status`
        // (default 200). A bogus value falls back to the default rather than a
        // 5xx — see `resolve_status`.
        StatusCode::from_u16(crate::resolve_status(self.status())).unwrap_or(StatusCode::OK)
    }

    fn error_response(&self) -> HttpResponse<BoxBody> {
        let mut builder = HttpResponse::build(self.status_code());
        #[cfg(feature = "rpc-status-header")]
        builder.insert_header((crate::rpc_status::X_RPC_STATUS, crate::rpc_status::ERROR));
        // Stash the type-erased error for middleware, matching the axum and poem
        // adapters. `error_response` only has `&self`, so this clones — the
        // `ApiErrorInner` box plus an `Arc` bump on the source chain, which is
        // not deep-copied.
        builder
            .extensions_mut()
            .insert(Arc::new(self.clone()) as Arc<dyn ApiErrorHandler>);
        builder.json(self.into_api_response::<()>())
    }
}

/// Pull the type-erased error back out of an actix response's extensions.
///
/// The actix counterpart of
/// [`response_get_api_error`](crate::response_get_api_error): `HttpResponse` is
/// not an `http::Response`, so it needs its own accessor.
///
/// Note that actix reaches `error_response` only for an error that is returned
/// as `Err` from a handler — one converted to a response by hand never passes
/// through the adapter and carries no extension.
pub fn response_get_api_error_actix<B>(response: &HttpResponse<B>) -> Option<Arc<dyn ApiErrorHandler>> {
    response.extensions().get::<Arc<dyn ApiErrorHandler>>().cloned()
}
