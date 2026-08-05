use actix_web::dev::Payload;
use actix_web::{FromRequest, HttpMessage, HttpRequest, ResponseError};
use std::future::{Ready, ready};
use uuid::Uuid;

/// The header [`Logger`](crate::Logger) reads an incoming request id from and
/// echoes back on the response.
pub const X_REQUEST_ID: actix_web::http::header::HeaderName =
    actix_web::http::header::HeaderName::from_static("x-request-id");

/// A per-request identifier, inserted by [`Logger`](crate::Logger) into the
/// request extensions and usable as a [`FromRequest`] extractor.
///
/// Always a UUID: a caller-supplied [`X_REQUEST_ID`] is adopted only when it
/// parses as one, so the type stays `Copy` and keeps deref'ing to [`Uuid`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestId(Uuid);

impl RequestId {
    pub(crate) fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    /// Reuse the caller's [`X_REQUEST_ID`] when it is a valid UUID, else generate
    /// a fresh one.
    ///
    /// Requiring a UUID is deliberate: it keeps one id format across services and
    /// stops a caller from injecting unbounded or malicious text into every log
    /// line that records `request_id`. Services that must echo an arbitrary
    /// caller-chosen token should carry it separately.
    pub(crate) fn from_headers(headers: &actix_web::http::header::HeaderMap) -> Self {
        headers
            .get(&X_REQUEST_ID)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| Uuid::parse_str(value.trim()).ok())
            .map_or_else(Self::generate, Self)
    }

    /// The underlying [`Uuid`].
    pub fn into_uuid(self) -> Uuid {
        self.0
    }

    /// Render as a header value. Infallible: a hyphenated UUID is always ASCII,
    /// so this formats into a fixed-size buffer and cannot fail.
    pub fn to_header_value(self) -> actix_web::http::header::HeaderValue {
        let mut buffer = Uuid::encode_buffer();
        let rendered: &str = self.0.hyphenated().encode_lower(&mut buffer);
        actix_web::http::header::HeaderValue::from_str(rendered)
            .expect("a hyphenated UUID is always a valid header value")
    }
}

impl From<Uuid> for RequestId {
    fn from(id: Uuid) -> Self {
        Self(id)
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
