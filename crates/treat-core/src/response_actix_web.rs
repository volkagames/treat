use crate::{ApiResponse, ResponseData};
use actix_web::body::BoxBody;
use actix_web::{HttpRequest, HttpResponse, Responder};

// Only `Serialize` (via `ResponseData`) is needed to write the body, as in the
// axum and poem adapters; no `Debug` bound.
impl<T, Meta> Responder for ApiResponse<T, Meta>
where
    T: ResponseData,
    Meta: ResponseData,
{
    type Body = BoxBody;

    fn respond_to(self, _: &HttpRequest) -> HttpResponse<Self::Body> {
        // The header tracks `errors[]`, not the status line, which stays 200 here.
        let mut builder = HttpResponse::Ok();
        #[cfg(feature = "rpc-status-header")]
        builder.insert_header((crate::rpc_status::X_RPC_STATUS, self.rpc_status()));
        builder.json(&self)
    }
}
