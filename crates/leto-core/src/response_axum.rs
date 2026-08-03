use crate::{ApiResponse, ResponseData};
use axum::{Json, response};

impl<T: ResponseData, Meta: ResponseData> response::IntoResponse for ApiResponse<T, Meta> {
    fn into_response(self) -> response::Response {
        // The header tracks `errors[]`, not the status line, which stays 200 here.
        #[cfg(feature = "rpc-status-header")]
        let outcome = self.rpc_status();
        #[allow(unused_mut)]
        let mut response = Json(self).into_response();
        #[cfg(feature = "rpc-status-header")]
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(crate::rpc_status::X_RPC_STATUS),
            axum::http::HeaderValue::from_static(outcome),
        );
        response
    }
}
