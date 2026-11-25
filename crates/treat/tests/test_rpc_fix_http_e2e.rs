//! The client against the server over a real socket, for what the RPC review
//! fixed: a form input under `GET`/`DELETE` travels in the query string, a
//! query value that is not UTF-8 is named by its key, and an event that does
//! not serialize ends a subscription where it stands.
#![cfg(all(feature = "rpc-server", feature = "rpc-client", feature = "derive"))]

use axum::Router;
use axum::extract::Request;
use axum::http::header;
use axum::response::Response;
use axum::routing::{delete, get, post};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use treat::ApiError;
use treat::rpc::client::{ClientError, RpcClient};
use treat::rpc::server::Refusal;
use treat::rpc::{HttpBodyKind, HttpSpec, QueryField, RuntimeCodes};
use validator::Validate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, treat::ApiErrorCode, treat::rpc::WireEnum)]
enum ErrorCode {
    #[code("things::invalid")]
    InvalidBody,
    #[code("internal_error")]
    InternalError,
}

impl RuntimeCodes for ErrorCode {
    const INVALID_BODY: Self = Self::InvalidBody;
    const INTERNAL_ERROR: Self = Self::InternalError;
}

type Rt = treat::rpc::Runtime<ErrorCode>;
type Handled = Result<Response, Refusal>;
type Failed = ApiError<ErrorCode>;
type Called<T> = Result<T, ClientError<ErrorCode>>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Validate)]
struct Filter {
    name: String,
    n: i64,
    #[serde(default)]
    tags: Vec<String>,
}

static FIELDS: &[QueryField] = &[
    QueryField::str("name"),
    QueryField::int("n"),
    QueryField::str("tags").list(),
];

static FORM_GET: HttpSpec = HttpSpec::get("/form").fields(FIELDS).body(HttpBodyKind::Form).bare();
static FORM_DELETE: HttpSpec = HttpSpec::delete("/form").fields(FIELDS).body(HttpBodyKind::Form).bare();
static FORM_POST: HttpSpec = HttpSpec::post("/form").fields(FIELDS).body(HttpBodyKind::Form).bare();
static FORM_ANY: HttpSpec = HttpSpec::any("/form/any")
    .fields(FIELDS)
    .body(HttpBodyKind::Form)
    .bare();

/// What a form operation read, and how the request carried it.
#[derive(Debug, Deserialize, Serialize)]
struct Seen {
    input: Filter,
    method: String,
    content_type: Option<String>,
    query: Option<String>,
}

/// A form operation answering with what it read and how it arrived.
async fn form(request: Request) -> Handled {
    let method = request.method().to_string();
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let query = request.uri().query().map(str::to_string);
    let spec = match method.as_str() {
        "GET" => &FORM_GET,
        "DELETE" => &FORM_DELETE,
        _ => &FORM_POST,
    };
    let input: Filter = Rt::http_input(request, spec).await?;
    let seen = Seen {
        input,
        method,
        content_type,
        query,
    };
    Ok(Rt::respond_http(spec, Ok::<_, Failed>(seen)))
}

async fn form_any(request: Request) -> Handled {
    let method = request.method().to_string();
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let query = request.uri().query().map(str::to_string);
    let input: Filter = Rt::http_input(request, &FORM_ANY).await?;
    let seen = Seen {
        input,
        method,
        content_type,
        query,
    };
    Ok(Rt::respond_http(&FORM_ANY, Ok::<_, Failed>(seen)))
}

fn router() -> Router {
    Router::new()
        .route("/form", get(form).post(form))
        .route("/form", delete(form))
        .route("/form/any", axum::routing::any(form_any))
        .route("/form/post", post(form))
}

async fn serve_router(router: Router) -> RpcClient {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free local port");
    let address = listener.local_addr().expect("a bound address");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("the server runs");
    });
    RpcClient::new(format!("http://{address}"))
}

fn filter() -> Filter {
    Filter {
        name: "a b&c".to_string(),
        n: 7,
        tags: vec!["x".to_string(), "y z".to_string()],
    }
}

#[tokio::test]
async fn a_form_input_under_get_or_delete_travels_in_the_query_string() {
    let client = serve_router(router()).await;
    for (spec, method) in [(&FORM_GET, "GET"), (&FORM_DELETE, "DELETE")] {
        let seen: Called<Seen> = client.call_http(spec, &filter()).await;
        let seen = seen.expect("the call succeeds");
        assert_eq!(seen.method, method);
        assert_eq!(seen.input, filter(), "{method}");
        assert_eq!(seen.content_type, None, "{method}: no body, so no body type");
        assert!(
            seen.query.as_deref().is_some_and(|query| query.contains("n=7")),
            "{method}: {:?}",
            seen.query
        );
    }
}

#[tokio::test]
async fn a_form_input_under_a_method_with_a_body_is_still_the_body() {
    let client = serve_router(router()).await;
    for spec in [&FORM_POST, &FORM_ANY] {
        let seen: Called<Seen> = client.call_http(spec, &filter()).await;
        let seen = seen.expect("the call succeeds");
        assert_eq!(seen.method, "POST");
        assert_eq!(seen.input, filter());
        assert_eq!(seen.content_type.as_deref(), Some("application/x-www-form-urlencoded"));
        assert_eq!(seen.query, None);
    }
}

#[tokio::test]
async fn an_empty_list_is_not_sent_and_reads_back_as_its_default() {
    let client = serve_router(router()).await;
    let input = Filter {
        tags: Vec::new(),
        ..filter()
    };
    let seen: Called<Seen> = client.call_http(&FORM_GET, &input).await;
    let seen = seen.expect("the call succeeds");
    assert_eq!(seen.input, input);
    assert!(
        seen.query.as_deref().is_some_and(|query| !query.contains("tags")),
        "{:?}",
        seen.query
    );
}

/// The status and body of a `GET` the client would not send.
async fn get_raw(client: &RpcClient, path_and_query: &str) -> (u16, Value) {
    let response = client
        .http()
        .get(format!("{}{path_and_query}", client.base_url()))
        .send()
        .await
        .expect("the server answers");
    let status = response.status().as_u16();
    let bytes = response.bytes().await.expect("the body arrives");
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn a_query_value_that_is_not_utf8_is_named_by_its_key() {
    let client = serve_router(router()).await;
    let (status, json) = get_raw(&client, "/form?name=caf%E9&n=1").await;
    assert_eq!(status, 400, "{json}");
    assert_eq!(
        json.pointer("/errors/0/code"),
        Some(&json!("things::invalid")),
        "{json}"
    );
    assert_eq!(
        json.pointer("/errors/0/source/parameter"),
        Some(&json!("name")),
        "{json}"
    );

    // A key that is not UTF-8 has no name to give.
    let (status, json) = get_raw(&client, "/form?caf%E9=x&name=a&n=1").await;
    assert_eq!(status, 400, "{json}");
    assert_eq!(json.pointer("/errors/0/source"), None, "{json}");
}

#[cfg(feature = "rpc-stream")]
mod events {
    use super::*;
    use treat::rpc::client::StreamError;
    use treat::rpc::futures_util::StreamExt;
    use treat::rpc::server::{event_stream, sse_response};

    /// An event whose second value refuses to serialize.
    struct Fragile(u32);

    impl Serialize for Fragile {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            if self.0 == 2 {
                return Err(serde::ser::Error::custom("two does not serialize"));
            }
            serializer.serialize_u32(self.0)
        }
    }

    /// The events a little apart, as a live subscription sends them: the
    /// answer's head and the first event are out before the second fails.
    async fn fragile() -> Response {
        let events = [Fragile(1), Fragile(2), Fragile(3)];
        let spaced = treat::rpc::futures_util::stream::iter(events).then(|event| {
            async move {
                if event.0 > 1 {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
                event
            }
        });
        sse_response(event_stream(spaced))
    }

    #[tokio::test]
    async fn an_event_that_does_not_serialize_ends_the_stream_there() {
        let client = serve_router(Router::new().route("/fragile", get(fragile))).await;
        let opened: Called<_> = client
            .subscribe::<_, u32, ErrorCode>("/fragile", &[], &treat::rpc::client::NoInput)
            .await;
        let events: Vec<Result<u32, StreamError>> = opened.expect("the stream opens").collect().await;
        assert!(matches!(events.first(), Some(Ok(1))), "{events:?}");
        assert!(
            matches!(events.get(1), Some(Err(StreamError::Transport(_)))),
            "{events:?}"
        );
        assert_eq!(events.len(), 2, "nothing after the failure: {events:?}");
    }
}
