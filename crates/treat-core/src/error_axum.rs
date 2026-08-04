use crate::{ApiError, ApiErrorCode, ApiErrorHandler, NoData};
use axum::http::StatusCode;
use std::sync::Arc;

impl<C: ApiErrorCode> axum::response::IntoResponse for ApiError<C> {
    fn into_response(self) -> axum::response::Response {
        // Reads the transport status set via `with_status`/`with_code_status`
        // (default 200), matching the actix and poem adapters. A bogus value
        // falls back to the default rather than a 5xx — see `resolve_status`.
        let status = StatusCode::from_u16(crate::resolve_status(self.status())).unwrap_or(StatusCode::OK);
        // Same builder as the actix adapter, so both frameworks emit identical JSON.
        let failure = self.into_api_response::<NoData>();
        let mut response = (status, axum::Json(failure)).into_response();
        #[cfg(feature = "rpc-status-header")]
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(crate::rpc_status::X_RPC_STATUS),
            axum::http::HeaderValue::from_static(crate::rpc_status::ERROR),
        );
        response
            .extensions_mut()
            .insert(Arc::new(self) as Arc<dyn ApiErrorHandler>);
        response
    }
}

/// Pull the type-erased error back out of an axum response's extensions.
///
/// Returns `Some` only for a response built from an [`ApiError`] — the adapter
/// stashes it on the way out so middleware and observability layers can read the
/// `code`, `status` and raise `location` without knowing the concrete code type.
///
/// The actix and poem adapters stash the same value; reach it with
/// [`response_get_api_error_actix`](crate::response_get_api_error_actix) /
/// [`response_get_api_error_poem`](crate::response_get_api_error_poem).
pub fn response_get_api_error<ResBody>(
    response: &axum::response::Response<ResBody>,
) -> Option<&Arc<dyn ApiErrorHandler>> {
    response.extensions().get::<Arc<dyn ApiErrorHandler>>()
}
