use actix_web::dev::Payload;
use actix_web::{FromRequest, HttpMessage, HttpRequest, ResponseError};
use std::future::{Ready, ready};
use tracing::Span;

#[derive(Clone)]
pub struct RootSpan(Span);

impl RootSpan {
    pub(crate) fn new(span: Span) -> Self {
        Self(span)
    }
}

impl std::ops::Deref for RootSpan {
    type Target = Span;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<RootSpan> for Span {
    fn from(r: RootSpan) -> Self {
        r.0
    }
}

impl FromRequest for RootSpan {
    type Error = RootSpanExtractionError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        ready(
            req.extensions()
                .get::<RootSpan>()
                .cloned()
                .ok_or(RootSpanExtractionError { _priv: () }),
        )
    }
}

#[derive(Debug)]
pub struct RootSpanExtractionError {
    // It turns out that a unit struct has a public constructor!
    // Therefore adding fields to it (either public or private) later on
    // is an API breaking change.
    // Therefore we are adding a dummy private field that the compiler is going
    // to optimise away to make sure users cannot construct this error
    // manually in their own code.
    _priv: (),
}

impl ResponseError for RootSpanExtractionError {}

impl std::fmt::Display for RootSpanExtractionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to retrieve the root span from request-local storage.")
    }
}

impl std::error::Error for RootSpanExtractionError {}
