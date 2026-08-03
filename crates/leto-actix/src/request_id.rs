use actix_web::dev::Payload;
use actix_web::{FromRequest, HttpMessage, HttpRequest, ResponseError};
use std::future::{Ready, ready};
use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
pub struct RequestId(Uuid);

impl RequestId {
    pub(crate) fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::ops::Deref for RequestId {
    type Target = Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<RequestId> for Uuid {
    fn from(r: RequestId) -> Self {
        r.0
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromRequest for RequestId {
    type Error = RequestIdExtractionError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        ready(
            req.extensions()
                .get::<RequestId>()
                .copied()
                .ok_or(RequestIdExtractionError { _priv: () }),
        )
    }
}

#[derive(Debug)]
pub struct RequestIdExtractionError {
    _priv: (),
}

impl ResponseError for RequestIdExtractionError {}

impl std::fmt::Display for RequestIdExtractionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to retrieve request id from request-local storage.")
    }
}

impl std::error::Error for RequestIdExtractionError {}
