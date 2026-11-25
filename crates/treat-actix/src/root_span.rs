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

/// The [`RootSpan`] extractor's rejection: the request did not pass through the
/// [`Logger`](crate::Logger). See its [`ResponseError`] impl for the response.
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

/// Rendered as a `treat` envelope with code `root_span_unavailable` and status `500`, like
/// any other `ApiError`: the extractor ran without the [`Logger`](crate::Logger)
/// in front of it, which is a wiring bug of the service, not a client error.
impl ResponseError for RootSpanExtractionError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        actix_web::http::StatusCode::INTERNAL_SERVER_ERROR
    }

    fn error_response(&self) -> actix_web::HttpResponse {
        treat_core::error("root_span_unavailable")
            .with_message(self.to_string())
            .with_status(500)
            .error_response()
    }
}

impl std::fmt::Display for RootSpanExtractionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to retrieve the root span from request-local storage.")
    }
}

impl std::error::Error for RootSpanExtractionError {}
