use crate::{ApiResponse, ResponseData};
use poem::web::Json;
use poem::{IntoResponse, Response};

// poem's `IntoResponse` is `Send`, so the payload types must be `Send` too.
impl<T: ResponseData + Send, Meta: ResponseData + Send> IntoResponse for ApiResponse<T, Meta> {
    fn into_response(self) -> Response {
        // The header tracks `errors[]`, not the status line, which stays 200 here.
        #[cfg(feature = "rpc-status-header")]
        let outcome = self.rpc_status();
        #[allow(unused_mut)]
        let mut response = Json(self).into_response();
        #[cfg(feature = "rpc-status-header")]
        response.headers_mut().insert(
            poem::http::HeaderName::from_static(crate::rpc_status::X_RPC_STATUS),
            poem::http::HeaderValue::from_static(outcome),
        );
        response
    }
}
