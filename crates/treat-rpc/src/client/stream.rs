//! Subscriptions over Server-Sent Events and raw byte bodies (features
//! `client` and `stream`, `treat`'s `rpc-client` and `rpc-stream`).

use super::call::{RpcStatus, envelope_failure, json_body, receive, refusal, rpc_status, send, transport};
use super::query::{input_fields, to_query};
use super::{ClientError, ClientResult, RpcClient};
use crate::{QueryField, media};
use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt;
use std::pin::Pin;

/// A stream of events, which is what a `@subscription` answers with.
///
/// Each item is one event or the reason the stream stopped being readable.
/// Opening the subscription is a separate, earlier failure: a server that
/// refuses answers with the envelope, so that arrives as [`ClientError`].
pub type EventStream<T> = Pin<Box<dyn Stream<Item = Result<T, StreamError>> + Send>>;

/// Why a subscription stopped before the server closed it.
///
/// Non-exhaustive for the reason [`ClientError`] is.
#[derive(Debug)]
#[non_exhaustive]
pub enum StreamError {
    /// The connection dropped mid-stream. The stream ends after reporting it.
    Transport(String),
    /// A frame arrived that was not the declared event type, and the stream
    /// goes on with the next frame; or the server sent more than
    /// [`SSE_BUFFER_LIMIT`](RpcClient::SSE_BUFFER_LIMIT) bytes without ending
    /// a line or an event, and the stream ends after reporting it.
    Decode(String),
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamError::Transport(m) => write!(f, "stream transport error: {m}"),
            StreamError::Decode(m) => write!(f, "could not decode event: {m}"),
        }
    }
}

impl std::error::Error for StreamError {}

impl RpcClient {
    /// The most bytes a subscription holds while waiting for a line or an
    /// event to end: 16 MiB.
    ///
    /// A bound on the client's memory rather than on what a server may send:
    /// an event is held until its blank line arrives, so a server that never
    /// sends one would otherwise grow the buffer until the process runs out.
    /// Past it the stream reports [`StreamError::Decode`] and ends.
    pub const SSE_BUFFER_LIMIT: usize = 16 << 20;

    /// The most bytes of a raw answer declared `application/json` read to tell
    /// a refusal from the operation's own JSON: 64 KiB.
    ///
    /// A refusal is a short envelope, so an answer still going past this is
    /// the operation's own and is handed back as it arrives, the bytes read so
    /// far first. Bounded so a large JSON download is not buffered whole
    /// before the caller sees any of it.
    pub const RAW_PEEK_LIMIT: usize = 64 << 10;

    /// Open a subscription over Server-Sent Events.
    ///
    /// `EventSource` semantics without `EventSource`: the input travels in the
    /// query string because the request is a `GET`, spelled as `fields` — the
    /// table the server reads it back with — says, and each `data` of an event
    /// is one JSON event of the declared type. Dropping the returned stream
    /// closes the connection; unlike a browser's `EventSource`, nothing
    /// reconnects on its own.
    ///
    /// Only a `2xx` answer of type `text/event-stream` opens the stream. Any
    /// other is read as a refusal: the envelope's `errors[]` is
    /// [`ClientError::Api`], and a body without one — a proxy's HTML page —
    /// is [`ClientError::Decode`] under a `2xx` status and
    /// [`ClientError::Status`] under any other.
    pub async fn subscribe<I, O, C>(
        &self,
        path: &str,
        fields: &[QueryField],
        input: &I,
    ) -> ClientResult<EventStream<O>, C>
    where
        I: Serialize + ?Sized,
        O: DeserializeOwned + Send + 'static,
    {
        let input = input_fields(input).map_err(ClientError::Encode)?;
        let query = to_query(input, fields).map_err(ClientError::Encode)?;
        let url = self.url(&format!("{path}{query}"));
        let request = self.http.get(url).header(reqwest::header::ACCEPT, "text/event-stream");
        let response = send(request).await?;

        // A refusal answers with the envelope rather than with a stream, so it
        // is reported here instead of as an event that never arrives.
        let refused = !response.status().is_success()
            || rpc_status(&response) == Some(RpcStatus::Error)
            || !has_media_type(&response, "text/event-stream");
        if refused {
            return Err(refusal(response).await);
        }
        Ok(sse_stream(response.bytes_stream(), Self::SSE_BUFFER_LIMIT))
    }

    /// Call an operation whose answer is the bytes rather than the envelope:
    /// `POST` with the input as a JSON body (`Content-Type: application/json`).
    ///
    /// The whole response is returned, so the headers that describe the body —
    /// `Content-Disposition`, `Content-Type` — are readable, and a long one can
    /// be read incrementally instead of buffered. A refusal is told from the
    /// bytes as [`raw_request`](Self::raw_request) describes.
    ///
    /// Takes no field table: the input travels as a JSON body, which needs
    /// none.
    pub async fn raw<I, C>(&self, path: &str, input: &I) -> ClientResult<reqwest::Response, C>
    where
        I: Serialize + ?Sized,
    {
        let request = json_body(self.http.post(self.url(path)), input)?;
        let response = send(request).await?;
        bytes_answer(response).await
    }

    /// Send a body of bytes, with the input's other fields as query parameters
    /// spelled as `fields` — the table the server reads them back with — says.
    ///
    /// The payload has the one body, which is why everything else is in the
    /// URL; the input type leaves its bytes field out of what it serializes.
    /// `content_type` says what the bytes are: the SDL lists what the operation
    /// accepts, and only the caller knows which of them it is sending. One that
    /// is not a valid header value is [`ClientError::Encode`].
    ///
    /// The answer is the operation's own bytes, returned as the response they
    /// arrived in, or a refusal, which is read from the envelope:
    ///
    /// - any status other than `2xx` is a refusal;
    /// - an `x-rpc-status` header (`treat`'s `rpc-status-header`) decides when
    ///   present: `error` is a refusal, `ok` the bytes;
    /// - without one, an answer of type `application/json` is read up to
    ///   [`RAW_PEEK_LIMIT`](Self::RAW_PEEK_LIMIT), and is a refusal if it ends
    ///   within that and is an envelope with `errors[]` — for an operation
    ///   whose own answer is JSON this is the only way to tell. Otherwise it is
    ///   handed back rebuilt: its status, headers, extensions (`remote_addr`
    ///   among them) and URL as they arrived, and a body of the bytes read
    ///   followed by the rest, still streaming. Any other type is the bytes,
    ///   unread.
    pub async fn raw_request<I, C>(
        &self,
        path: &str,
        fields: &[QueryField],
        input: &I,
        body: impl Into<reqwest::Body>,
        content_type: &str,
    ) -> ClientResult<reqwest::Response, C>
    where
        I: Serialize + ?Sized,
    {
        let response = self.send_raw_request(path, fields, input, body, content_type).await?;
        bytes_answer(response).await
    }

    /// A raw request whose answer is an ordinary envelope — an upload
    /// reporting what it wrote: [`raw_request`](Self::raw_request)'s request,
    /// with the answer read like [`call`](Self::call)'s.
    pub async fn raw_request_json<I, O, C>(
        &self,
        path: &str,
        fields: &[QueryField],
        input: &I,
        body: impl Into<reqwest::Body>,
        content_type: &str,
    ) -> ClientResult<O, C>
    where
        I: Serialize + ?Sized,
        O: DeserializeOwned,
    {
        let response = self.send_raw_request(path, fields, input, body, content_type).await?;
        receive(response, true).await
    }

    /// Send a raw request and return whatever answered, unread.
    async fn send_raw_request<I, C>(
        &self,
        path: &str,
        fields: &[QueryField],
        input: &I,
        body: impl Into<reqwest::Body>,
        content_type: &str,
    ) -> ClientResult<reqwest::Response, C>
    where
        I: Serialize + ?Sized,
    {
        let input = input_fields(input).map_err(ClientError::Encode)?;
        let query = to_query(input, fields).map_err(ClientError::Encode)?;
        let url = self.url(&format!("{path}{query}"));
        let request = self
            .http
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body);
        send(request).await
    }
}

/// The answer of an operation that answers bytes on success: the response, or
/// the refusal it carries; see [`RpcClient::raw_request`] for how the two are
/// told apart.
async fn bytes_answer<C>(response: reqwest::Response) -> ClientResult<reqwest::Response, C> {
    if !response.status().is_success() {
        return Err(refusal(response).await);
    }
    match rpc_status(&response) {
        Some(RpcStatus::Error) => return Err(refusal(response).await),
        Some(RpcStatus::Ok) => return Ok(response),
        None => {}
    }
    if !has_media_type(&response, "application/json") {
        return Ok(response);
    }

    // JSON, and nothing but the body says whose: read the start of it, and
    // hand it back rebuilt if it is not a refusal.
    peek_json(response, RpcClient::RAW_PEEK_LIMIT).await
}

/// A JSON raw answer told from a refusal by its first `limit` bytes: a body
/// that ends within them and is an envelope with `errors[]` is the refusal;
/// anything else is the response, rebuilt around the bytes already read.
async fn peek_json<C>(mut response: reqwest::Response, limit: usize) -> ClientResult<reqwest::Response, C> {
    let mut prefix: Vec<u8> = Vec::new();
    let mut ended = false;
    while !ended && prefix.len() <= limit {
        let chunk = response.chunk().await.map_err(transport)?;
        match chunk {
            Some(chunk) => prefix.extend_from_slice(&chunk),
            None => ended = true,
        }
    }
    if ended && prefix.len() <= limit {
        let failure = envelope_failure(&prefix);
        if let Some(errors) = failure {
            return Err(ClientError::Api(errors));
        }
    }
    rebuild(response, prefix, ended)
}

/// `response` again, with `prefix` — the bytes already read from it — in
/// front of the rest of its body, which keeps streaming unless `ended` says
/// there is none.
///
/// Everything the caller can read of a response survives: status, version,
/// headers, extensions (where `remote_addr` lives) and the URL, which
/// `reqwest` keeps apart from the extensions and takes back from one of them.
fn rebuild<C>(response: reqwest::Response, prefix: Vec<u8>, ended: bool) -> ClientResult<reqwest::Response, C> {
    let status = response.status();
    let version = response.version();
    let headers = response.headers().clone();
    let extensions = response.extensions().clone();
    let url = response.url().clone();
    let body = match ended {
        true => reqwest::Body::from(prefix),
        false => {
            let read = futures_util::stream::iter([Ok::<Bytes, reqwest::Error>(Bytes::from(prefix))]);
            reqwest::Body::wrap_stream(read.chain(response.bytes_stream()))
        }
    };

    // The URL travels as an extension only `reqwest`'s builder can make, which
    // `Response::from` takes back out. A builder with nothing else set cannot
    // fail, so the error arm is never taken.
    let with_url = reqwest::ResponseBuilderExt::url(http::Response::builder(), url)
        .body(body)
        .map_err(|e| ClientError::Decode(e.to_string()))?;
    let (mut parts, body) = with_url.into_parts();
    parts.status = status;
    parts.version = version;
    parts.headers = headers;
    // The copied extensions go in first and the URL over them, so a copy
    // cannot replace it.
    let url_only = std::mem::replace(&mut parts.extensions, extensions);
    parts.extensions.extend(url_only);
    Ok(reqwest::Response::from(http::Response::from_parts(parts, body)))
}

/// Whether a response's `Content-Type` is `expected`: its media type alone,
/// before any `;` parameter, compared without regard to case. Exact, so
/// `application/jsonl` is not `application/json`.
fn has_media_type(response: &reqwest::Response, expected: &str) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| media::is(value, expected))
}

/// State threaded through the unfolded SSE stream.
struct SseState<S> {
    bytes: Pin<Box<S>>,
    parser: SseParser,
    /// Set once the stream has reported a failure it cannot read past, so it
    /// ends there instead of reporting the same failure for every later poll.
    failed: bool,
}

/// Turn the body of an open SSE response into a stream of decoded events.
///
/// Generic over the byte stream so the framing can be exercised without a
/// connection. Each chunk yields the events its arrival completed — none, one
/// or several — and a trailing partial event when the server closes the stream
/// is incomplete by definition, so it is dropped rather than parsed, as the
/// SSE specification says.
pub(crate) fn sse_stream<O, S, B, E>(bytes: S, limit: usize) -> EventStream<O>
where
    O: DeserializeOwned + Send + 'static,
    S: Stream<Item = Result<B, E>> + Send + 'static,
    B: AsRef<[u8]>,
    E: fmt::Display,
{
    let state = SseState {
        bytes: Box::pin(bytes),
        parser: SseParser::new(limit),
        failed: false,
    };
    let batches = futures_util::stream::unfold(state, |mut state| {
        async move {
            if state.failed {
                return None;
            }
            let chunk = state.bytes.next().await;
            match chunk {
                Some(Ok(chunk)) => {
                    let fed = state.parser.feed(chunk.as_ref());
                    let mut events: Vec<Result<O, StreamError>> = Vec::with_capacity(fed.len());
                    for item in fed {
                        match item {
                            Ok(data) => {
                                events
                                    .push(serde_json::from_str(&data).map_err(|e| StreamError::Decode(e.to_string())));
                            }
                            Err(overflow) => {
                                state.failed = true;
                                events.push(Err(StreamError::Decode(overflow)));
                            }
                        }
                    }
                    Some((events, state))
                }
                Some(Err(e)) => {
                    state.failed = true;
                    Some((vec![Err(StreamError::Transport(e.to_string()))], state))
                }
                None => None,
            }
        }
    });
    Box::pin(batches.flat_map(futures_util::stream::iter))
}

/// An incremental parser of the `text/event-stream` format, as the HTML
/// specification defines it, reduced to what a subscription reads: the `data`
/// of each event.
///
/// Fed chunk by chunk, it looks at every byte once: a line is collected until
/// its end arrives, so a chunk boundary anywhere — inside a line, between the
/// `\r` and `\n` of one line end — changes nothing, and nothing already looked
/// at is searched again.
#[derive(Debug)]
pub(crate) struct SseParser {
    /// The bytes of the line being read, its end not yet seen.
    line: Vec<u8>,
    /// The `data` of the event being read: each `data` line's value followed by
    /// a `\n`, as the specification builds it.
    data: String,
    /// The last line ended with `\r`, so a `\n` arriving next is the rest of
    /// that line end rather than an empty line.
    after_cr: bool,
    /// The first line has been read, so a byte order mark is no longer
    /// skipped.
    started: bool,
    /// The most bytes `line` and `data` may hold together.
    limit: usize,
}

/// The UTF-8 byte order mark, which the specification skips at the start of
/// the stream.
const BOM: &[u8] = b"\xEF\xBB\xBF";

impl SseParser {
    pub(crate) const fn new(limit: usize) -> Self {
        Self {
            line: Vec::new(),
            data: String::new(),
            after_cr: false,
            started: false,
            limit,
        }
    }

    /// Read `chunk`: the `data` of every event it completed, in order, then
    /// — if the parser held more than its limit — why the stream cannot be
    /// read further, after which the rest of `chunk` is not read.
    pub(crate) fn feed(&mut self, chunk: &[u8]) -> Vec<Result<String, String>> {
        let mut events = Vec::new();
        let mut rest = chunk;
        while let Some(&first) = rest.first() {
            if self.after_cr {
                self.after_cr = false;
                if first == b'\n' {
                    rest = rest.get(1..).unwrap_or_default();
                    continue;
                }
            }
            let end = rest.iter().position(|byte| matches!(byte, b'\r' | b'\n'));
            match end.and_then(|end| rest.split_at_checked(end)) {
                Some((line, tail)) => {
                    self.line.extend_from_slice(line);
                    self.after_cr = tail.first() == Some(&b'\r');
                    rest = tail.get(1..).unwrap_or_default();
                    let event = self.end_line();
                    events.extend(event.map(Ok));
                }
                None => {
                    self.line.extend_from_slice(rest);
                    rest = &[];
                }
            }
            let overflow = self.overflow();
            if let Some(overflow) = overflow {
                events.push(Err(overflow));
                rest = &[];
            }
        }
        events
    }

    /// Why the stream cannot be read further, once the line and the event
    /// being read hold more than the limit.
    fn overflow(&self) -> Option<String> {
        let held = self.line.len().saturating_add(self.data.len());
        (held > self.limit).then(|| {
            format!(
                "the event stream sent more than {} bytes without ending an event",
                self.limit
            )
        })
    }

    /// Process the line collected in `line`: the `data` of the event it ends,
    /// if it is the blank line that ends one carrying data.
    fn end_line(&mut self) -> Option<String> {
        let mut line = std::mem::take(&mut self.line);
        if !self.started {
            self.started = true;
            if line.starts_with(BOM) {
                line.drain(..BOM.len());
            }
        }
        if line.is_empty() {
            return self.dispatch();
        }
        let text = String::from_utf8_lossy(&line);
        // A comment, `: keep-alive` among them.
        if text.starts_with(':') {
            return None;
        }
        // A line without a colon is a field with an empty value.
        let (name, value) = text.split_once(':').unwrap_or((&text, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        // `event`, `id` and `retry` say nothing a subscription reads.
        if name == "data" {
            self.data.push_str(value);
            self.data.push('\n');
        }
        None
    }

    /// End the event being read: its `data` without the last `\n`, or `None`
    /// when it had no `data` line at all.
    fn dispatch(&mut self) -> Option<String> {
        let mut data = std::mem::take(&mut self.data);
        if data.is_empty() {
            return None;
        }
        data.pop();
        Some(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

    /// The `data` of every event `chunks` complete, fed one after another.
    fn feed(chunks: &[&[u8]]) -> Vec<Result<String, String>> {
        let mut parser = SseParser::new(RpcClient::SSE_BUFFER_LIMIT);
        chunks.iter().flat_map(|chunk| parser.feed(chunk)).collect()
    }

    fn data(chunks: &[&[u8]]) -> Vec<String> {
        feed(chunks)
            .into_iter()
            .map(|item| item.unwrap_or_else(|e| e))
            .collect()
    }

    #[test]
    fn events_end_at_a_blank_line_of_any_line_ending() {
        assert_eq!(
            data(&[b"data: 1\n\ndata: 2\r\n\r\ndata: 3\r\rdata: 4\n\nda"]),
            ["1", "2", "3", "4"]
        );
        // Mixed endings: `\r\n` then `\n` is a line end and a blank line.
        assert_eq!(data(&[b"data: 1\r\n\ndata: 2\n\r\n"]), ["1", "2"]);
    }

    #[test]
    fn an_event_waits_for_its_blank_line() {
        let mut parser = SseParser::new(64);
        assert!(parser.feed(b"data: {\"a\":").is_empty());
        assert!(parser.feed(b"1}\n").is_empty());
        assert_eq!(parser.feed(b"\n"), [Ok(r#"{"a":1}"#.to_string())]);
    }

    #[test]
    fn a_line_end_split_between_chunks_is_one_line_end() {
        // `\r` ends the line at once; the `\n` arriving next belongs to it.
        assert_eq!(data(&[b"data: 1\r", b"\n", b"\r", b"\n"]), ["1"]);
        assert_eq!(data(&[b"data: 1\r", b"\ndata: 2\r", b"\r"]), ["1\n2"]);
        assert_eq!(data(&[b"data: 1\r", b"\r"]), ["1"]);
        // Byte by byte.
        let stream = b"data: a\r\ndata: b\r\n\r\ndata: c\n\n";
        let bytes: Vec<&[u8]> = stream.chunks(1).collect();
        assert_eq!(data(&bytes), ["a\nb", "c"]);
    }

    #[test]
    fn data_lines_join_with_a_newline() {
        assert_eq!(data(&[b"event: tick\ndata: [1,\ndata:2]\nid: 7\n\n"]), ["[1,\n2]"]);
        assert_eq!(data(&[b"data: a\r\ndata: b\r\n\r\n"]), ["a\nb"]);
        assert_eq!(
            data(&[b"data:  two spaces\n\n"]),
            [" two spaces"],
            "one space is stripped"
        );
    }

    #[test]
    fn a_data_line_without_a_colon_is_empty_data() {
        assert_eq!(data(&[b"data\n\n"]), [""]);
        assert_eq!(data(&[b"data:\n\n"]), [""]);
        assert_eq!(data(&[b"data\ndata: x\n\n"]), ["\nx"]);
    }

    #[test]
    fn comments_and_events_without_data_are_not_events() {
        assert_eq!(
            data(&[b": keep-alive\n\nretry: 100\n\nevent: x\n\n\xff\xfe\n\ndata: x\n\n"]),
            ["x"]
        );
        assert_eq!(data(&[b"\n\n\n"]), Vec::<String>::new());
        assert_eq!(data(&[b"datum: x\n\n"]), Vec::<String>::new(), "only `data` is data");
    }

    #[test]
    fn a_byte_order_mark_at_the_start_is_skipped() {
        assert_eq!(data(&[b"\xEF\xBB\xBFdata: 1\n\n"]), ["1"]);
        assert_eq!(data(&[b"\xEF", b"\xBB", b"\xBFdata: 1\n\n"]), ["1"]);
        // Only at the start: later it is part of a field name.
        assert_eq!(data(&[b"data: 1\n\n\xEF\xBB\xBFdata: 2\n\n"]), ["1"]);
    }

    #[test]
    fn more_than_the_limit_without_an_event_is_an_error() {
        let mut parser = SseParser::new(8);
        assert_eq!(parser.feed(b"data: 1\n\n"), [Ok("1".to_string())]);
        let fed = parser.feed(b"data: 12345678");
        assert!(matches!(fed.as_slice(), [Err(_)]), "a long line: {fed:?}");

        let mut parser = SseParser::new(8);
        let fed = parser.feed(b"data: 1\n\ndata: 1234\ndata: 5678\n\ndata: 2\n\n");
        assert!(
            matches!(fed.as_slice(), [Ok(first), Err(_)] if first == "1"),
            "many short lines, and nothing read after: {fed:?}"
        );
    }

    /// Every item `stream` has ready, without an executor: the test streams
    /// never wait.
    fn ready_items<O>(mut stream: EventStream<O>) -> Vec<Result<O, StreamError>> {
        let mut items = Vec::new();
        while let Some(Some(item)) = stream.next().now_or_never() {
            items.push(item);
        }
        items
    }

    fn chunks(chunks: Vec<&'static [u8]>) -> impl Stream<Item = Result<&'static [u8], String>> + Send + 'static {
        futures_util::stream::iter(chunks.into_iter().map(Ok))
    }

    #[test]
    fn events_decode_across_chunk_boundaries() {
        let stream = sse_stream::<u32, _, _, _>(
            chunks(vec![
                b"data: 1",
                b"\n",
                b"\ndata: 2\n\ndata: 3\r\n",
                b"\r\n: ping\n\ndata: 4\n\ndata: 5",
            ]),
            RpcClient::SSE_BUFFER_LIMIT,
        );
        let events: Vec<Option<u32>> = ready_items(stream).into_iter().map(Result::ok).collect();
        // The trailing `data: 5` never got its blank line.
        assert_eq!(events, [Some(1), Some(2), Some(3), Some(4)]);
    }

    #[test]
    fn a_bad_event_is_reported_and_the_stream_goes_on() {
        let stream = sse_stream::<u32, _, _, _>(chunks(vec![b"data: x\n\ndata: 2\n\n"]), 64);
        let items = ready_items(stream);
        assert_eq!(items.len(), 2, "{items:?}");
        assert!(matches!(items.first(), Some(Err(StreamError::Decode(_)))), "{items:?}");
        assert!(matches!(items.get(1), Some(Ok(2))), "{items:?}");
    }

    #[test]
    fn an_overflow_ends_the_stream() {
        let stream = sse_stream::<u32, _, _, _>(chunks(vec![b"data: 1\n\ndata: 123456789", b"\n\ndata: 2\n\n"]), 8);
        let items = ready_items(stream);
        assert_eq!(items.len(), 2, "{items:?}");
        assert!(matches!(items.first(), Some(Ok(1))), "{items:?}");
        assert!(
            matches!(items.get(1), Some(Err(StreamError::Decode(m))) if m.contains("more than 8 bytes")),
            "{items:?}"
        );
    }

    #[test]
    fn a_transport_failure_ends_the_stream() {
        let bytes = futures_util::stream::iter([
            Ok(&b"data: 1\n\n"[..]),
            Err("reset".to_string()),
            Ok(&b"data: 2\n\n"[..]),
        ]);
        let items = ready_items(sse_stream::<u32, _, _, _>(bytes, 64));
        assert_eq!(items.len(), 2, "{items:?}");
        assert!(matches!(items.first(), Some(Ok(1))), "{items:?}");
        assert!(
            matches!(items.get(1), Some(Err(StreamError::Transport(m))) if m == "reset"),
            "{items:?}"
        );
    }
}
