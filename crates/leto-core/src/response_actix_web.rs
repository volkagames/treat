use crate::{ApiResponse, ResponseData};
use actix_web::body::BoxBody;
use actix_web::{HttpRequest, HttpResponse, Responder};
use std::fmt::Debug;

impl<T, Meta> Responder for ApiResponse<T, Meta>
where
    T: Debug + ResponseData,
    Meta: Debug + ResponseData,
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
