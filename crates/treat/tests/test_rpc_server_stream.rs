//! Streaming and raw bodies on the server: `raw_body`'s type check and limit,
//! Server-Sent Events framing, `RawBody` responses and `RawRequest`.
#![cfg(all(feature = "rpc-server", feature = "rpc-stream", feature = "derive"))]

mod rpc_server;

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, header};
use axum::response::Response;
use axum::routing::post;
use rpc_server::{ErrorCode, INVALID, Rt, code, read};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use tower::ServiceExt;
use treat::ApiError;
use treat::rpc::futures_util::io::AsyncReadExt;
use treat::rpc::futures_util::{StreamExt, stream};
use treat::rpc::server::{
    QueryField,
    QueryKind,
    RAW_BODY_IGNORED_HEADERS,
    RAW_BODY_LIMIT,
    RawBody,
    RawRequest,
    Refusal,
    event_stream,
    sse_response,
};
use validator::Validate;

type Handled = Result<Response, Refusal>;

async fn text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body should buffer");
    String::from_utf8(bytes.to_vec()).expect("the body should be text")
}

#[test]
fn the_buffered_limit_is_eight_mebibytes() {
    assert_eq!(RAW_BODY_LIMIT, 8 << 20);
}

/// A request with `body` declared as `content_type`.
fn typed(content_type: Option<&str>, body: impl Into<Body>) -> Request {
    let mut request = Request::builder().method("POST").uri("/");
    if let Some(content_type) = content_type {
        request = request.header(header::CONTENT_TYPE, content_type);
    }
    request.body(body.into()).expect("a valid request")
}

const CSV: &[&str] = &["text/csv", "application/json"];

#[tokio::test]
async fn raw_body_reads_up_to_its_limit() {
    let read_back = Rt::raw_body(typed(Some("text/csv"), "abcd"), CSV, 4).await.ok();
    assert_eq!(read_back, Some(b"abcd".to_vec()));

    let refused = Rt::raw_body(typed(Some("text/csv"), "abcde"), CSV, 4)
        .await
        .expect_err("over the limit");
    let (status, json) = read(refused).await;
    assert_eq!((status, code(&json)), (413, Some(INVALID)), "{json}");
    assert_eq!(
        json.pointer("/errors/0/message"),
        Some(&json!("request body is larger than 4 bytes"))
    );
}

#[tokio::test]
async fn a_raw_body_that_breaks_off_keeps_the_envelope_s_status() {
    let broken = stream::iter([
        Ok(treat::rpc::bytes::Bytes::from_static(b"ab")),
        Err(std::io::Error::other("connection reset")),
    ]);
    let refused = Rt::raw_body(typed(Some("text/csv"), Body::from_stream(broken)), CSV, 64)
        .await
        .expect_err("a body that broke off");
    let (status, json) = read(refused).await;
    assert_eq!(
        (status, code(&json)),
        (200, Some(INVALID)),
        "not a size problem: {json}"
    );
    assert_eq!(
        json.pointer("/errors/0/message"),
        Some(&json!("request body cannot be read: connection reset"))
    );
}

#[tokio::test]
async fn raw_body_reads_only_a_declared_type() {
    for content_type in ["text/csv", "Text/CSV; charset=utf-8", "application/json"] {
        let read_back = Rt::raw_body(typed(Some(content_type), "a"), CSV, 4).await.ok();
        assert_eq!(read_back, Some(b"a".to_vec()), "{content_type}");
    }
    // What a cross-origin form posts, a near miss, and no type at all.
    for content_type in [
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
        Some("multipart/form-data; boundary=x"),
        Some("text/csvx"),
        Some("application/problem+json"),
        None,
    ] {
        let refused = Rt::raw_body(typed(content_type, "a"), CSV, 4)
            .await
            .expect_err("an undeclared type");
        let (status, json) = read(refused).await;
        assert_eq!((status, code(&json)), (415, Some(INVALID)), "{content_type:?}: {json}");
        assert_eq!(json.pointer("/errors/0/source/header"), Some(&json!("content-type")));
        assert_eq!(
            json.pointer("/errors/0/message"),
            Some(&json!(
                "the request body must be declared by `Content-Type` as one of `text/csv`, `application/json`"
            ))
        );
    }
}

#[tokio::test]
async fn a_declared_range_takes_every_subtype() {
    let images: &[&str] = &["image/*"];
    assert!(Rt::raw_body(typed(Some("image/png"), "a"), images, 4).await.is_ok());
    assert!(Rt::raw_body(typed(Some("image/webp"), "a"), images, 4).await.is_ok());
    assert!(Rt::raw_body(typed(Some("text/plain"), "a"), images, 4).await.is_err());

    let anything: &[&str] = &["*/*"];
    assert!(Rt::raw_body(typed(Some("text/plain"), "a"), anything, 4).await.is_ok());

    let (status, json) = read(Rt::require_content_type(&axum::http::HeaderMap::new(), &[]).expect_err("none")).await;
    assert_eq!((status, code(&json)), (415, Some(INVALID)));
    assert_eq!(
        json.pointer("/errors/0/message"),
        Some(&json!("the operation accepts no request body"))
    );
}

/// A request without `Content-Type` is accepted exactly where the list already
/// admits a type a cross-origin page posts without a preflight; elsewhere the
/// refusal is what makes the browser ask first.
#[tokio::test]
async fn a_missing_type_is_accepted_only_where_a_form_post_already_is() {
    enum Sent {
        Absent,
        Typed(&'static str),
        NotUtf8,
    }
    let cases: &[(&[&str], Sent, bool)] = &[
        (&["application/octet-stream"], Sent::Absent, false),
        (
            &["application/octet-stream"],
            Sent::Typed("application/octet-stream"),
            true,
        ),
        (&["text/csv"], Sent::Absent, false),
        (&["image/*"], Sent::Absent, false),
        (&["*/*"], Sent::Absent, true),
        (&["text/*"], Sent::Absent, true),
        (&["multipart/*"], Sent::Absent, true),
        (&["text/plain"], Sent::Absent, true),
        (&["application/x-www-form-urlencoded"], Sent::Absent, true),
        (&["multipart/form-data", "application/octet-stream"], Sent::Absent, true),
        (&["*/*"], Sent::NotUtf8, false),
        (&["*/*"], Sent::Typed(""), false),
        (&[], Sent::Absent, false),
    ];
    for (declared, sent, accepted) in cases {
        let mut headers = axum::http::HeaderMap::new();
        let shown = match sent {
            Sent::Absent => "absent".to_string(),
            Sent::Typed(value) => {
                headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(value));
                format!("{value:?}")
            }
            Sent::NotUtf8 => {
                let value = HeaderValue::from_bytes(b"text/\xffplain").expect("a header value, though not UTF-8");
                headers.insert(header::CONTENT_TYPE, value);
                "not UTF-8".to_string()
            }
        };
        let result = Rt::require_content_type(&headers, declared);
        let refused = match result {
            Ok(()) => {
                assert!(accepted, "{declared:?} {shown}: accepted");
                continue;
            }
            Err(refused) => refused,
        };
        assert!(!accepted, "{declared:?} {shown}: refused");
        let (status, json) = read(refused).await;
        assert_eq!(
            (status, code(&json)),
            (415, Some(INVALID)),
            "{declared:?} {shown}: {json}"
        );
        assert_eq!(json.pointer("/errors/0/source/header"), Some(&json!("content-type")));
    }

    // The buffered path goes through the same check.
    let anything: &[&str] = &["*/*"];
    let read_back = Rt::raw_body(typed(None, "a"), anything, 4).await.ok();
    assert_eq!(read_back, Some(b"a".to_vec()));
    let octets: &[&str] = &["application/octet-stream"];
    assert!(Rt::raw_body(typed(None, "a"), octets, 4).await.is_err());
}

#[derive(Debug, Serialize)]
struct Tick {
    n: u32,
}

#[tokio::test]
async fn events_are_one_json_data_frame_each() {
    let events = event_stream(stream::iter([Tick { n: 1 }, Tick { n: 2 }]));
    let response = Rt::respond_events(Ok::<_, ApiError<ErrorCode>>(events));
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );
    assert_eq!(text(response).await, "data: {\"n\":1}\n\ndata: {\"n\":2}\n\n");
}

/// The next frame of an event stream's body and when it went out, measured
/// from `start` on the paused clock.
async fn next_frame(
    frames: &mut axum::body::BodyDataStream,
    start: tokio::time::Instant,
) -> (Option<String>, Duration) {
    let frame = frames.next().await;
    let text = frame
        .and_then(Result::ok)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    (text, start.elapsed())
}

/// The keep-alive comment goes out after 30 quiet seconds, and the clock
/// restarts after each event: on tokio's paused clock, which jumps straight
/// to the next timer whenever the test has nothing else to do, so the half
/// minutes pass in no time.
#[tokio::test(start_paused = true)]
async fn a_quiet_stream_is_kept_alive_every_thirty_seconds() {
    let one_then_quiet = stream::once(async {
        tokio::time::sleep(Duration::from_secs(10)).await;
        Tick { n: 1 }
    })
    .chain(stream::pending());
    let start = tokio::time::Instant::now();
    let mut frames = sse_response(event_stream(one_then_quiet))
        .into_body()
        .into_data_stream();

    let event = next_frame(&mut frames, start).await;
    assert_eq!(
        event,
        (Some("data: {\"n\":1}\n\n".to_string()), Duration::from_secs(10))
    );

    let keep_alive = Some(": keep-alive\n\n".to_string());
    let first = next_frame(&mut frames, start).await;
    assert_eq!(
        first,
        (keep_alive.clone(), Duration::from_secs(40)),
        "30 s after the event"
    );
    let second = next_frame(&mut frames, start).await;
    assert_eq!(
        second,
        (keep_alive, Duration::from_secs(70)),
        "30 s after the last ping"
    );
}

#[tokio::test]
async fn a_subscription_that_fails_answers_with_the_envelope() {
    let failed = Err::<treat::rpc::server::EventStream<Tick>, _>(ApiError::from(ErrorCode::NotFound));
    let (status, json) = read(Rt::respond_events(failed)).await;
    assert_eq!((status, code(&json)), (treat::DEFAULT_ERROR_STATUS, Some("not_found")));
}

#[tokio::test]
async fn a_raw_body_is_its_bytes_under_its_headers() {
    let raw = RawBody::new("text/csv", "a,b\n")
        .header(HeaderName::from_static("x-rows"), HeaderValue::from_static("1"))
        .header(header::SET_COOKIE, HeaderValue::from_static("a=1"))
        .header(header::SET_COOKIE, HeaderValue::from_static("b=2"))
        .header(header::CONTENT_TYPE, HeaderValue::from_static("text/html"))
        .header(header::CONTENT_LENGTH, HeaderValue::from_static("999"))
        .attachment("q\"1\r\nSet-Cookie: x=y;é.csv");
    let response = Rt::respond_raw(Ok::<_, ApiError<ErrorCode>>(raw));
    assert_eq!(response.status(), 200);
    let header = |name| response.headers().get(name).and_then(|value| value.to_str().ok());
    let all = |name| {
        response
            .headers()
            .get_all(name)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        all(header::CONTENT_TYPE.as_str()),
        ["text/csv"],
        "the field's, not the header's"
    );
    assert!(all(header::CONTENT_LENGTH.as_str()).is_empty(), "the body says it");
    assert_eq!(
        all(header::SET_COOKIE.as_str()),
        ["a=1", "b=2"],
        "a name twice is sent twice"
    );
    assert_eq!(header("x-rows"), Some("1"));
    assert_eq!(
        header(header::CONTENT_DISPOSITION.as_str()),
        Some("attachment; filename=\"q_1__Set-Cookie: x=y;_.csv\""),
        "a quote, a line break or a non-ASCII character cannot end the header",
    );
    assert_eq!(text(response).await, "a,b\n");
}

#[tokio::test]
async fn an_unusable_content_type_is_the_service_s_failure() {
    let raw = RawBody::new("text/plain\nx-injected: 1", "body");
    let (status, json) = read(Rt::raw_response(raw)).await;
    assert_eq!((status, code(&json)), (200, Some("rpc::internal_error")), "{json}");
    assert_eq!(
        json.pointer("/errors/0/message"),
        Some(&json!("`text/plain\nx-injected: 1` is not a usable content type"))
    );

    let failed = Err::<RawBody, _>(ApiError::from(ErrorCode::NotFound).with_status(404));
    assert_eq!(read(Rt::respond_raw(failed)).await.0, 404);
}

#[derive(Debug, Deserialize, Serialize, Validate)]
struct Upload {
    #[validate(length(max = 8))]
    name: String,
    #[serde(default)]
    body: Vec<u8>,
}

static UPLOAD_FIELDS: &[QueryField] = &[QueryField {
    name: "name",
    kind: QueryKind::Str,
    list: false,
}];

/// What `@raw(request: [...])` declares.
const UPLOAD_TYPES: &[&str] = &["application/octet-stream"];

/// A buffered `@raw(request:)` handler: the query string, then the body, then
/// the rules.
async fn upload(request: Request) -> Handled {
    let mut input: Upload = Rt::input_from_query(request.uri().query(), UPLOAD_FIELDS)?;
    input.body = Rt::raw_body(request, UPLOAD_TYPES, 8).await?;
    Rt::validate(&input)?;
    Ok(Rt::respond_bare(Ok::<_, ApiError<ErrorCode>>(
        json!({ "name": input.name, "len": input.body.len() }),
    )))
}

/// A streamed `@raw(request:, stream: true)` handler: the type checked, the
/// body unread.
async fn stream_upload(raw: RawRequest) -> Handled {
    Rt::require_content_type(raw.headers(), UPLOAD_TYPES)?;
    let input: Upload = Rt::query_input(raw.query(), UPLOAD_FIELDS)?;
    let length = raw.content_length();
    let kind = raw.content_type().map(str::to_string);
    let mut body = Vec::new();
    let mut reader = raw.into_reader();
    let read = reader.read_to_end(&mut body).await;
    let summary = json!({
        "name": input.name,
        "declared": length,
        "type": kind,
        "read": read.ok(),
        "body": String::from_utf8_lossy(&body),
    });
    Ok(Rt::respond_bare(Ok::<_, ApiError<ErrorCode>>(summary)))
}

async fn send(uri: &str, body: &'static str) -> (u16, serde_json::Value) {
    send_as(uri, "application/octet-stream", body).await
}

async fn send_as(uri: &str, content_type: &str, body: &'static str) -> (u16, serde_json::Value) {
    let router = Router::new()
        .route("/upload", post(upload))
        .route("/stream", post(stream_upload));
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, body.len())
        .body(Body::from(body))
        .expect("a valid request");
    let response = router.oneshot(request).await.expect("the router is infallible");
    read(response).await
}

#[tokio::test]
async fn a_buffered_raw_request() {
    assert_eq!(
        send("/upload?name=a", "12345678").await,
        (200, json!({ "name": "a", "len": 8 }))
    );

    let (status, json) = send("/upload?name=a", "123456789").await;
    assert_eq!((status, code(&json)), (413, Some(INVALID)), "over the limit: {json}");

    let (status, json) = send("/upload?name=very+long+name", "1").await;
    assert_eq!(
        (status, code(&json)),
        (200, Some(INVALID)),
        "validated once the body is in: {json}"
    );
    assert_eq!(json.pointer("/errors/0/source/pointer"), Some(&json!("/name")));
}

#[tokio::test]
async fn a_streamed_raw_request_reaches_the_service_unread() {
    let (status, json) = send("/stream?name=s", "a body larger than any buffered limit").await;
    assert_eq!(status, 200, "{json}");
    assert_eq!(
        json,
        json!({
            "name": "s",
            "declared": 37,
            "type": "application/octet-stream",
            "read": 37,
            "body": "a body larger than any buffered limit",
        })
    );

    let (status, json) = send("/stream?name=far+too+long", "x").await;
    assert_eq!(
        (status, code(&json)),
        (200, Some(INVALID)),
        "the query input is still validated: {json}"
    );
}

/// What a page on another origin can post without a preflight, with its
/// visitor's cookies: refused before the service sees a byte, buffered or not.
#[tokio::test]
async fn a_raw_request_under_an_undeclared_type_is_refused() {
    for uri in ["/upload?name=a", "/stream?name=a"] {
        for content_type in ["text/plain", "application/x-www-form-urlencoded", "multipart/form-data"] {
            let (status, json) = send_as(uri, content_type, "x").await;
            assert_eq!(
                (status, code(&json)),
                (415, Some(INVALID)),
                "{uri} {content_type}: {json}"
            );
        }
    }
}

/// With `rpc-status-header`, a success that is not an envelope says `ok` too,
/// and a header of the service's own cannot say otherwise.
#[cfg(feature = "rpc-status-header")]
#[tokio::test]
async fn raw_and_event_successes_say_ok() {
    let status = |response: &Response| {
        response
            .headers()
            .get("x-rpc-status")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    };
    let events = event_stream(stream::iter([Tick { n: 1 }]));
    let response = Rt::respond_events(Ok::<_, ApiError<ErrorCode>>(events));
    assert_eq!(status(&response).as_deref(), Some("ok"));

    let raw = RawBody::new("text/csv", "a").header(
        HeaderName::from_static("x-rpc-status"),
        HeaderValue::from_static("error"),
    );
    let response = Rt::respond_raw(Ok::<_, ApiError<ErrorCode>>(raw));
    assert_eq!(status(&response).as_deref(), Some("ok"));
    assert_eq!(response.headers().get_all("x-rpc-status").iter().count(), 1);

    let failed = Rt::respond_raw(Err::<RawBody, _>(ApiError::from(ErrorCode::NotFound)));
    assert_eq!(status(&failed).as_deref(), Some("error"));
}

/// A service's `x-rpc-status` is not sent, with the feature or without: the
/// header is the runtime's account of the outcome.
#[tokio::test]
async fn a_service_s_status_header_is_not_sent() {
    assert!(RAW_BODY_IGNORED_HEADERS.contains(&"x-rpc-status"));
    let raw = RawBody::new("text/csv", "a").header(
        HeaderName::from_static("x-rpc-status"),
        HeaderValue::from_static("error"),
    );
    let response = Rt::respond_raw(Ok::<_, ApiError<ErrorCode>>(raw));
    let sent: Vec<_> = response.headers().get_all("x-rpc-status").iter().cloned().collect();
    let expected: Vec<HeaderValue> = match cfg!(feature = "rpc-status-header") {
        true => vec![HeaderValue::from_static("ok")],
        false => Vec::new(),
    };
    assert_eq!(sent, expected);
}
