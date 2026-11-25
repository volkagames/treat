//! Streaming and raw bodies (features `server` and `stream`): what a
//! `@subscription` and a `@raw` operation answer with or read.

use super::handler::mark_ok;
use super::{Refusal, http_status};
use crate::{Runtime, RuntimeCodes};
use axum::body::Body;
use axum::extract::{FromRequest, Request};
use axum::http::request::Parts;
use axum::http::{Extensions, HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::{StreamExt, TryStreamExt};
use serde::Serialize;
use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;
use treat_core::{ApiError, ApiErrorCode};

/// A stream of events, which is what a `@subscription` answers with.
///
/// Boxed rather than generic: a trait method cannot name an opaque return type,
/// and the handler does nothing with the stream but poll it.
pub type EventStream<T> = Pin<Box<dyn futures_core::Stream<Item = T> + Send>>;

/// Box any stream of events into the shape a subscription returns.
pub fn event_stream<T, S>(stream: S) -> EventStream<T>
where
    S: futures_core::Stream<Item = T> + Send + 'static,
{
    Box::pin(stream)
}

/// Bound on a buffered `@raw(request:)` body, in bytes.
///
/// One number for every raw operation: the schema says which fields carry a
/// body but not how large one may be, so this is a floor under the server's
/// memory rather than a statement about any particular upload. An operation
/// needing a tighter bound states it as a `@length` on the field, which is
/// checked after the body is read; one needing a larger body streams it instead
/// of buffering.
pub const RAW_BODY_LIMIT: usize = 8 * 1024 * 1024;

/// Render a stream of events as Server-Sent Events.
///
/// One JSON event per `data:` frame, which is the framing an `EventSource`
/// reads.
///
/// An idle stream would otherwise sit silent until its next event, and an idle
/// proxy in between would cut it, so a `: keep-alive` comment goes out every
/// 30 seconds — an SSE comment is ignored by `EventSource`. The timer resets
/// after every event, so pings go out only while the stream is actually quiet.
///
/// With feature `rpc-status-header` the response carries `x-rpc-status: ok`,
/// as every successful envelope does: the stream opened.
pub fn sse_response<T: Serialize + Send + 'static>(stream: EventStream<T>) -> Response {
    let frames = stream.map(|event| Event::default().json_data(event));
    let response = Sse::new(frames)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(30)).text("keep-alive"))
        .into_response();
    mark_ok(response)
}

/// A response body that is bytes rather than the JSON envelope.
///
/// What a `@raw(response:)` operation answers with. The headers describing the
/// body — `Content-Disposition` above all — are part of the answer rather than
/// metadata about it, so they are carried here instead of being derived from
/// the bytes.
///
/// Turned into a response by [`Runtime::raw_response`] rather than
/// `IntoResponse`: a content type that is not a valid header value is refused
/// with the registry's [`RuntimeCodes::INTERNAL_ERROR`], which only the
/// registry-aware runtime can name.
#[derive(Debug, Clone)]
pub struct RawBody {
    /// The `Content-Type` of `body`.
    pub content_type: String,
    /// The response body.
    pub body: Vec<u8>,
    /// Response headers beyond the content type, sent in order; a name listed
    /// twice is sent twice (two `Set-Cookie`s, say).
    ///
    /// Three names are the runtime's and are not sent from here:
    /// `Content-Type` (the field above says it), `Content-Length` (the body
    /// does) and `x-rpc-status` (the outcome, which a service's header must not
    /// contradict; see [`RAW_BODY_IGNORED_HEADERS`]).
    pub headers: Vec<(HeaderName, HeaderValue)>,
}

/// The headers [`Runtime::raw_response`] leaves out of a [`RawBody`]'s
/// `headers`: the runtime writes them itself, from the body and the outcome,
/// and a second value from the service would contradict the first.
pub const RAW_BODY_IGNORED_HEADERS: [&str; 3] = ["content-type", "content-length", "x-rpc-status"];

impl RawBody {
    /// `body` with the given content type and no other headers.
    pub fn new(content_type: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            content_type: content_type.into(),
            body: body.into(),
            headers: Vec::new(),
        }
    }

    /// Add a response header, after any already added (of this name too).
    ///
    /// Typed rather than string-keyed, so a header this cannot represent is a
    /// compile error at the call site instead of a surprise in the response.
    /// One of [`RAW_BODY_IGNORED_HEADERS`] is kept here but not sent.
    #[must_use]
    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.push((name, value));
        self
    }

    /// Offer the body as a download named `filename`.
    ///
    /// The name is sanitised rather than trusted: it often comes from a record
    /// title, and a quote or a newline in one would otherwise end the header
    /// value early and let the rest be read as another header.
    #[must_use]
    pub fn attachment(self, filename: &str) -> Self {
        let safe: String = filename
            .chars()
            .map(|c| {
                match c {
                    '"' | '\\' | '\r' | '\n' => '_',
                    c if c.is_control() || !c.is_ascii() => '_',
                    c => c,
                }
            })
            .collect();
        match HeaderValue::from_str(&format!("attachment; filename=\"{safe}\"")) {
            Ok(value) => self.header(header::CONTENT_DISPOSITION, value),
            // Every unrepresentable character was replaced above, so this is
            // unreachable; dropping the header beats refusing the download.
            Err(_) => self,
        }
    }
}

/// A `@raw(request:, stream: true)` body, handed to the service unread.
///
/// **No size limit is applied; the service method receiving this owns it.**
/// Nothing in a generated router bounds a streamed body — [`RAW_BODY_LIMIT`]
/// covers buffered ones only. Skipping the buffer saves memory, but an
/// unbounded body still fills a disk and holds a connection, so the method must
/// set the bound: refuse on [`Self::content_length`] before reading, and count
/// what it reads, since a chunked request declares no length at all.
///
/// The request's headers and extensions come along — `ConnectInfo` included,
/// when the router is served with it — because the service may need them to
/// decide whether to read the body at all.
///
/// An extractor that never refuses: it takes the request apart without reading
/// the body, so a handler may still refuse the caller before anything is read.
/// Nor does it check the `Content-Type`; the handler does, first, with
/// [`Runtime::require_content_type`], as [`Runtime::raw_body`] does for a
/// buffered body:
///
/// ```ignore
/// async fn ingest<S: FilesService>(State(service): State<S>, raw: RawRequest) -> Result<Response, Refusal> {
///     Rt::require_content_type(raw.headers(), &["text/csv"])?;
///     let input: IngestInput = Rt::query_input(raw.query(), INGEST_FIELDS)?;
///     Ok(Rt::respond(service.ingest(input, raw).await))
/// }
/// ```
#[derive(Debug)]
pub struct RawRequest {
    parts: Parts,
    body: Body,
}

impl RawRequest {
    /// The request's headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.parts.headers
    }

    /// The request's extensions: what the router and its layers attached.
    pub fn extensions(&self) -> &Extensions {
        &self.parts.extensions
    }

    /// The request's query string, which carries the operation's other input
    /// fields.
    pub fn query(&self) -> Option<&str> {
        self.parts.uri.query()
    }

    /// The declared `Content-Length`, or `None` when the request declares none
    /// or one that is not a number.
    pub fn content_length(&self) -> Option<u64> {
        self.parts
            .headers
            .get(header::CONTENT_LENGTH)?
            .to_str()
            .ok()?
            .parse()
            .ok()
    }

    /// The declared `Content-Type`, when it is text.
    pub fn content_type(&self) -> Option<&str> {
        self.parts.headers.get(header::CONTENT_TYPE)?.to_str().ok()
    }

    /// The body, unread.
    pub fn into_body(self) -> Body {
        self.body
    }

    /// The body as a byte reader, for a service that copies it somewhere.
    pub fn into_reader(self) -> impl futures_util::io::AsyncRead + Unpin + Send + 'static {
        self.body
            .into_data_stream()
            .map_err(std::io::Error::other)
            .into_async_read()
    }
}

impl From<Request> for RawRequest {
    fn from(request: Request) -> Self {
        let (parts, body) = request.into_parts();
        Self { parts, body }
    }
}

impl<S: Send + Sync> FromRequest<S> for RawRequest {
    type Rejection = Infallible;

    async fn from_request(request: Request, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self::from(request))
    }
}

/// Whether reading a body failed because it went past a length limit: the
/// one `to_bytes` applies, or one a layer wrapped the body in (`tower-http`'s
/// `RequestBodyLimitLayer`), anywhere in the error's chain of sources.
fn over_limit(error: &axum::Error) -> bool {
    let first = std::error::Error::source(error);
    std::iter::successors(first, |cause| cause.source()).any(|cause| cause.is::<http_body_util::LengthLimitError>())
}

impl<R: RuntimeCodes> Runtime<R> {
    /// Read a buffered `@raw(request: [...])` body: the bytes of `request`,
    /// declared as one of the media ranges `declared` and at most `limit`
    /// bytes long.
    ///
    /// The type is checked first, with [`Runtime::require_content_type`]
    /// (`415`), so nothing is read under a type the operation does not take;
    /// see [`Runtime::no_input`] for why that matters. A body longer than
    /// `limit` is refused at `413`, and one that cannot be read for another
    /// reason — the connection dropped — at the envelope's `200`. Every refusal
    /// carries [`RuntimeCodes::INVALID_BODY`].
    ///
    /// A bound is applied here rather than left to the service because the body
    /// is buffered to reach it: without one, the size of an upload would be the
    /// caller's choice rather than the server's.
    ///
    /// Takes the whole request, so the input's other fields are read from its
    /// query string first:
    ///
    /// ```ignore
    /// let mut input: UploadInput = Rt::input_from_query(request.uri().query(), UPLOAD_FIELDS)?;
    /// input.file = Rt::raw_body(request, &["text/csv"], RAW_BODY_LIMIT).await?;
    /// Rt::validate(&input)?;
    /// ```
    pub async fn raw_body(request: Request, declared: &[&str], limit: usize) -> Result<Vec<u8>, Refusal> {
        Self::require_content_type(request.headers(), declared)?;
        let read = axum::body::to_bytes(request.into_body(), limit).await;
        let error = match read {
            Ok(bytes) => return Ok(bytes.to_vec()),
            Err(error) => error,
        };
        if over_limit(&error) {
            let refused = Self::invalid_query(&format!("request body is larger than {limit} bytes"), None);
            return Err(http_status(refused, StatusCode::PAYLOAD_TOO_LARGE).into());
        }
        Err(Self::invalid_query(&format!("request body cannot be read: {error}"), None).into())
    }

    /// The response a [`RawBody`] describes: its bytes under its content type
    /// and headers, and with feature `rpc-status-header` `x-rpc-status: ok`.
    ///
    /// The headers are appended in order, so a name listed twice is sent twice;
    /// the [`RAW_BODY_IGNORED_HEADERS`] among them are left out, since the
    /// runtime writes those from the body and the outcome.
    ///
    /// A content type that is not a valid header value is the service's own
    /// bug rather than a bad request, so it is answered with
    /// [`Runtime::internal_failure`].
    pub fn raw_response(raw: RawBody) -> Response {
        let content_type = match HeaderValue::from_str(&raw.content_type) {
            Ok(value) => value,
            Err(_) => {
                return Self::internal_failure(&format!("`{}` is not a usable content type", raw.content_type));
            }
        };

        let mut response = Response::new(Body::from(raw.body));
        let headers = response.headers_mut();
        headers.insert(header::CONTENT_TYPE, content_type);
        let sent = raw
            .headers
            .into_iter()
            .filter(|(name, _)| !RAW_BODY_IGNORED_HEADERS.contains(&name.as_str()));
        for (name, value) in sent {
            headers.append(name, value);
        }
        mark_ok(response)
    }

    /// Answer with a `@raw(response:)` operation's result: the bytes, or the
    /// failure's envelope at the status it carries.
    pub fn respond_raw<C: ApiErrorCode>(result: Result<RawBody, ApiError<C>>) -> Response {
        match result {
            Ok(raw) => Self::raw_response(raw),
            Err(error) => error.into_response(),
        }
    }

    /// Answer with a `@subscription`'s result: its events as Server-Sent Events
    /// ([`sse_response`]), or the failure's envelope at the status it carries.
    pub fn respond_events<T: Serialize + Send + 'static, C: ApiErrorCode>(
        result: Result<EventStream<T>, ApiError<C>>,
    ) -> Response {
        match result {
            Ok(stream) => sse_response(stream),
            Err(error) => error.into_response(),
        }
    }
}
