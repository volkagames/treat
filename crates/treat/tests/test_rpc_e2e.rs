//! The client against the server over a real socket: what each half writes is
//! what the other reads, for every way an operation carries its input and its
//! answer.
//!
//! The server is an axum router on `127.0.0.1:0` built from the runtime's
//! helpers the way a generated one is; the client is `RpcClient`. Where a test
//! needs a status line the client does not expose, or a request the client
//! would never send, it asks with the same `reqwest` client directly.
#![cfg(all(
    feature = "rpc-server",
    feature = "rpc-client",
    feature = "rpc-stream",
    feature = "derive"
))]

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, RawQuery, Request};
use axum::http::request::Parts;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{any, get, post, put};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use treat::rpc::client::reqwest::{self, Method, header};
use treat::rpc::client::{ClientError, NoInput, RpcClient, StreamError};
use treat::rpc::futures_util::StreamExt;
use treat::rpc::futures_util::io::AsyncReadExt;
use treat::rpc::server::{Ctx, RawBody, RawRequest, Refusal, ResolveContext, event_stream};
use treat::rpc::{HttpBodyKind, HttpMethod, HttpSpec, QueryField, QueryKind, RuntimeCodes};
use treat::{ApiError, ApiResponse};
use validator::Validate;

/// A registry whose runtime codes are not spelled like identifiers, so a
/// test sees that the wire name — not `treat`'s own `invalid_body`, not the
/// variant's name — is what travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, treat::ApiErrorCode, treat::rpc::WireEnum)]
enum ErrorCode {
    #[code("things::invalid")]
    InvalidBody,
    #[code("INTERNAL_ERROR")]
    InternalError,
    #[code("not_found")]
    NotFound,
    #[code("unauthenticated")]
    Unauthenticated,
    #[code("forbidden")]
    Forbidden,
}

impl RuntimeCodes for ErrorCode {
    const INVALID_BODY: Self = Self::InvalidBody;
    const INTERNAL_ERROR: Self = Self::InternalError;
}

type Rt = treat::rpc::Runtime<ErrorCode>;
type Handled = Result<Response, Refusal>;
type Failed = ApiError<ErrorCode>;
type Called<T> = Result<T, ClientError<ErrorCode>>;

/// The router's body limit, which `json_input` answers `413` beyond.
const BODY_LIMIT: usize = 1024;

const fn field(name: &'static str, kind: QueryKind, list: bool) -> QueryField {
    QueryField { name, kind, list }
}

// ---- inputs ---------------------------------------------------------------

/// A field of every kind, lists included: what a query string or a form body
/// has to spell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Validate)]
struct Everything {
    s: String,
    i: i64,
    f: f64,
    b: bool,
    j: Value,
    o: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    ns: Vec<i64>,
    #[serde(default)]
    js: Vec<Value>,
}

static EVERYTHING: &[QueryField] = &[
    field("s", QueryKind::Str, false),
    field("i", QueryKind::Int, false),
    field("f", QueryKind::Float, false),
    field("b", QueryKind::Bool, false),
    field("j", QueryKind::Json, false),
    field("o", QueryKind::Str, false),
    field("tags", QueryKind::Str, true),
    field("ns", QueryKind::Int, true),
    field("js", QueryKind::Json, true),
];

fn everything(j: Value) -> Everything {
    Everything {
        s: "a b&c=d+e%/é😀?#".to_string(),
        i: -42,
        f: 1.5,
        b: true,
        j,
        o: None,
        tags: vec!["x".to_string(), "y z".to_string(), "".to_string()],
        ns: vec![1, 2],
        js: vec![json!("1"), json!({"k": [null]}), json!(3)],
    }
}

/// The JSON values a `Json` field must carry as themselves: a string that
/// reads as a number, an array, an object. Not `null`, which a query string
/// spells by leaving the field out, so only an optional field can hold it.
fn json_values() -> Vec<Value> {
    vec![
        json!("abc"),
        json!("123"),
        json!(""),
        json!("null"),
        json!([1, "two"]),
        json!({"k": {"n": 1}}),
        json!(2.5),
        json!(false),
    ]
}

/// Path parameters of three kinds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Validate)]
struct PathInput {
    name: String,
    n: i64,
    j: Value,
    rest: Option<String>,
}

static PATH_FIELDS: &[QueryField] = &[
    field("name", QueryKind::Str, false),
    field("n", QueryKind::Int, false),
    field("j", QueryKind::Json, false),
    field("rest", QueryKind::Str, false),
];

/// List fields carried in the path, one item each.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Validate)]
struct ListPath {
    tags: Vec<String>,
    ns: Vec<i64>,
    js: Vec<Value>,
}

static LIST_PATH_FIELDS: &[QueryField] = &[
    field("tags", QueryKind::Str, true),
    field("ns", QueryKind::Int, true),
    field("js", QueryKind::Json, true),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Validate)]
struct Rename {
    id: i64,
    #[validate(length(min = 1, message = "empty"))]
    name: String,
    #[serde(default)]
    padding: String,
}

static RENAME_FIELDS: &[QueryField] = &[field("id", QueryKind::Int, false), field("name", QueryKind::Str, false)];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Validate)]
struct Ticks {
    count: u32,
    #[serde(default)]
    fail: bool,
    label: Value,
}

static TICKS_FIELDS: &[QueryField] = &[
    field("count", QueryKind::Int, false),
    field("fail", QueryKind::Bool, false),
    field("label", QueryKind::Json, false),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Tick {
    n: u32,
    label: Value,
}

#[derive(Debug, Serialize, Deserialize, Validate)]
struct Report {
    format: String,
}

/// A raw request's input: the query fields, then the body the handler reads.
#[derive(Debug, Serialize, Deserialize, Validate)]
struct Convert {
    name: String,
    opts: Value,
    #[serde(default, skip_serializing)]
    body: Vec<u8>,
}

static CONVERT_FIELDS: &[QueryField] = &[
    field("name", QueryKind::Str, false),
    field("opts", QueryKind::Json, false),
];

/// What each raw request declares in `@raw(request: [...])`.
const CONVERT_TYPES: &[&str] = &["text/plain"];
const UPLOAD_TYPES: &[&str] = &["application/octet-stream"];
const STREAM_TYPES: &[&str] = &["text/*"];

/// The buffered limit of `/upload/small`, so a test can pass it cheaply.
const SMALL_RAW_LIMIT: usize = 16;

// ---- the `@http` operations, one `static` each, shared by both ends --------

const fn http(
    method: HttpMethod,
    path: &'static str,
    fields: &'static [QueryField],
    path_fields: &'static [&'static str],
    body: HttpBodyKind,
    envelope: bool,
) -> HttpSpec {
    HttpSpec {
        method,
        path,
        fields,
        path_fields,
        body,
        envelope,
    }
}

/// `@http(method: GET, path: "/echo", envelope: false)`
static ECHO: HttpSpec = http(HttpMethod::Get, "/echo", EVERYTHING, &[], HttpBodyKind::None, false);
/// `@http(method: POST, path: "/echo/form", body: FORM)`, enveloped.
static ECHO_FORM: HttpSpec = http(
    HttpMethod::Post,
    "/echo/form",
    EVERYTHING,
    &[],
    HttpBodyKind::Form,
    true,
);
/// `@http(method: GET, path: "/paths/{name}/{n}/{j}", envelope: false)`
static PATHS: HttpSpec = http(
    HttpMethod::Get,
    "/paths/{name}/{n}/{j}",
    PATH_FIELDS,
    &["name", "n", "j"],
    HttpBodyKind::None,
    false,
);
/// `@http(method: GET, path: "/lists/{tags}/{ns}/{js}", envelope: false)`
static LIST_PATHS: HttpSpec = http(
    HttpMethod::Get,
    "/lists/{tags}/{ns}/{js}",
    LIST_PATH_FIELDS,
    &["tags", "ns", "js"],
    HttpBodyKind::None,
    false,
);
/// `@http(method: PUT, path: "/items/{id}", body: JSON)`, enveloped.
static PUT_ITEM: HttpSpec = http(
    HttpMethod::Put,
    "/items/{id}",
    RENAME_FIELDS,
    &["id"],
    HttpBodyKind::Json,
    true,
);
/// `@http(method: GET, path: "/guarded", envelope: false)`
static GUARDED: HttpSpec = http(HttpMethod::Get, "/guarded", &[], &[], HttpBodyKind::None, false);
/// `@http(method: GET, path: "/broken", envelope: false)`
static BROKEN: HttpSpec = http(HttpMethod::Get, "/broken", &[], &[], HttpBodyKind::None, false);
/// `@http(method: ANY, path: "/any")`: sent as `GET`, the input in the query.
static ANY_QUERY: HttpSpec = http(HttpMethod::Any, "/any", RENAME_FIELDS, &[], HttpBodyKind::None, false);
/// `@http(method: ANY, path: "/any", body: FORM)`: sent as `POST`, the input
/// in a form body.
static ANY_FORM: HttpSpec = http(HttpMethod::Any, "/any", RENAME_FIELDS, &[], HttpBodyKind::Form, false);

// ---- the server -----------------------------------------------------------

/// A caller named by an `x-user` header.
#[derive(Debug)]
struct Session {
    user: String,
}

impl ResolveContext for Session {
    async fn from_parts(parts: &mut Parts) -> Result<Self, Refusal> {
        let user = parts.headers.get("x-user").and_then(|value| value.to_str().ok());
        match user {
            Some(user) => Ok(Self { user: user.to_string() }),
            None => Err(ApiError::from(ErrorCode::Unauthenticated).into()),
        }
    }
}

fn bare<T: Serialize>(value: T) -> Response {
    Rt::respond_bare(Ok::<_, Failed>(value))
}

fn enveloped<T: Serialize>(value: T) -> Response {
    Rt::respond(Ok::<ApiResponse<T>, Failed>(treat::success(value)))
}

/// A generated `@http` handler: read as the spec says, answered as it says.
async fn echo_query(request: Request) -> Handled {
    let input: Everything = Rt::http_input(request, &ECHO).await?;
    Ok(Rt::respond_http(&ECHO, Ok::<_, Failed>(input)))
}

async fn echo_form(request: Request) -> Handled {
    let input: Everything = Rt::http_input(request, &ECHO_FORM).await?;
    Ok(Rt::respond_http(&ECHO_FORM, Ok::<_, Failed>(treat::success(input))))
}

async fn paths(request: Request) -> Handled {
    let input: PathInput = Rt::http_input(request, &PATHS).await?;
    Ok(Rt::respond_http(&PATHS, Ok::<_, Failed>(input)))
}

async fn list_paths(request: Request) -> Handled {
    let input: ListPath = Rt::http_input(request, &LIST_PATHS).await?;
    Ok(Rt::respond_http(&LIST_PATHS, Ok::<_, Failed>(input)))
}

async fn put_item(request: Request) -> Handled {
    let input: Rename = Rt::http_input(request, &PUT_ITEM).await?;
    Ok(Rt::respond_http(&PUT_ITEM, Ok::<_, Failed>(treat::success(input))))
}

/// An `ANY` route: the method it was called with beside the input it read.
async fn any_method(request: Request) -> Handled {
    let method = request.method().to_string();
    // Either spec reads the request; the form one reads a body when there is
    // one.
    let input: Rename = Rt::http_input(request, &ANY_FORM).await?;
    Ok(bare(json!({ "method": method, "input": input })))
}

/// `@http(method: GET, path: "/guarded", envelope: false)` behind a session
/// and an access check.
async fn guarded(ctx: Result<Ctx<Session>, Refusal>) -> Handled {
    let ctx = Rt::http_session(ctx)?;
    let allowed = match ctx.user == "admin" {
        true => Ok(()),
        false => Err(ApiError::from(ErrorCode::Forbidden).into()),
    };
    Rt::http_access(allowed)?;
    Ok(bare(json!({ "user": ctx.user })))
}

/// `/broken`, failing.
async fn broken() -> Response {
    Rt::respond_http(&BROKEN, Err::<Value, _>(ApiError::from(ErrorCode::InternalError)))
}

/// An RPC operation with an input.
async fn rpc_rename(request: Request) -> Handled {
    let input: Rename = Rt::json_input(request).await?;
    Ok(enveloped(input))
}

/// An RPC operation that succeeds with nothing to say: `{"data":null}`.
async fn rpc_forget(request: Request) -> Handled {
    let _: Rename = Rt::json_input(request).await?;
    Ok(enveloped(()))
}

/// An RPC operation without one, guarded on the RPC side: a refusal at `200`.
async fn rpc_ping(Ctx(ctx): Ctx<Session>, request: Request) -> Handled {
    Rt::no_input(request).await?;
    Ok(enveloped(format!("pong {}", ctx.user)))
}

/// `@subscription` over a query string.
async fn ticks(RawQuery(query): RawQuery) -> Handled {
    let input: Ticks = Rt::query_input(query.as_deref(), TICKS_FIELDS)?;
    if input.fail {
        return Ok(Rt::respond_events(Err::<treat::rpc::server::EventStream<Tick>, _>(
            ApiError::from(ErrorCode::Forbidden),
        )));
    }
    let label = input.label;
    let events = (1..=input.count).map(move |n| {
        Tick {
            n,
            label: label.clone(),
        }
    });
    Ok(Rt::respond_events(Ok::<_, Failed>(event_stream(
        treat::rpc::futures_util::stream::iter(events),
    ))))
}

/// What a proxy in front of the service answers with: a page, at `200`.
async fn proxy_page() -> Html<&'static str> {
    Html("<html><body>Sign in to continue</body></html>")
}

/// `@raw(response: [...])`: bytes of several types, or a refusal.
async fn report(request: Request) -> Handled {
    let input: Report = Rt::json_input(request).await?;
    let raw = match input.format.as_str() {
        "csv" => Ok(RawBody::new("text/csv", "a,b\n1,2\n").attachment("report.csv")),
        "json" => Ok(RawBody::new("application/json; charset=utf-8", r#"{"rows":[1,2]}"#)),
        "jsonl" => Ok(RawBody::new("application/jsonl", "{\"errors\":[{\"code\":\"x\"}]}\n")),
        "json-errors" => Ok(RawBody::new("application/json", r#"{"errors":[{"code":"x"}]}"#)),
        _ => Err(ApiError::from(ErrorCode::NotFound)),
    };
    Ok(Rt::respond_raw(raw))
}

/// `@raw(request: [...], response: [...])`: the query, a buffered body, bytes
/// back.
async fn convert(request: Request) -> Handled {
    let mut input: Convert = Rt::input_from_query(request.uri().query(), CONVERT_FIELDS)?;
    input.body = Rt::raw_body(request, CONVERT_TYPES, treat::rpc::server::RAW_BODY_LIMIT).await?;
    Rt::validate(&input)?;
    if input.name == "missing" {
        return Ok(Rt::respond_raw(Err::<RawBody, _>(ApiError::from(ErrorCode::NotFound))));
    }
    let text = String::from_utf8_lossy(&input.body).to_uppercase();
    let summary = format!("{} {} {text}", input.name, input.opts);
    Ok(Rt::respond_raw(Ok::<_, Failed>(RawBody::new("text/plain", summary))))
}

/// `@raw(request: [...])` answering the envelope.
async fn upload(request: Request) -> Handled {
    upload_within(request, treat::rpc::server::RAW_BODY_LIMIT).await
}

/// `/upload` with a limit a test can pass.
async fn small_upload(request: Request) -> Handled {
    upload_within(request, SMALL_RAW_LIMIT).await
}

async fn upload_within(request: Request, limit: usize) -> Handled {
    let mut input: Convert = Rt::input_from_query(request.uri().query(), CONVERT_FIELDS)?;
    input.body = Rt::raw_body(request, UPLOAD_TYPES, limit).await?;
    if input.name == "missing" {
        return Ok(Rt::respond(Err::<ApiResponse<Value>, _>(ApiError::from(
            ErrorCode::NotFound,
        ))));
    }
    Ok(enveloped(
        json!({ "name": input.name, "opts": input.opts, "len": input.body.len() }),
    ))
}

/// `@raw(request: [...], stream: true)`: the body unread until the service
/// reads it.
async fn stream_upload(raw: RawRequest) -> Handled {
    Rt::require_content_type(raw.headers(), STREAM_TYPES)?;
    let input: Convert = Rt::query_input(raw.query(), CONVERT_FIELDS)?;
    let declared = raw.content_length();
    let content_type = raw.content_type().map(str::to_string);
    let mut body = Vec::new();
    let read = raw.into_reader().read_to_end(&mut body).await;
    Ok(enveloped(json!({
        "name": input.name,
        "declared": declared,
        "type": content_type,
        "read": read.ok(),
        "body": String::from_utf8_lossy(&body),
    })))
}

fn router() -> Router {
    Router::new()
        .route("/echo", get(echo_query))
        .route("/echo/form", post(echo_form))
        .route("/paths/{name}/{n}/{j}", get(paths))
        .route("/lists/{tags}/{ns}/{js}", get(list_paths))
        .route("/items/{id}", put(put_item))
        .route("/guarded", get(guarded))
        .route("/broken", get(broken))
        .route("/any", any(any_method))
        .route("/rpc/rename", post(rpc_rename))
        .route("/rpc/ping", post(rpc_ping))
        .route("/rpc/forget", post(rpc_forget))
        .route("/ticks", get(ticks))
        .route("/proxy", get(proxy_page).post(proxy_page))
        .route("/report", post(report))
        .route("/convert", post(convert))
        .route("/upload", post(upload))
        .route("/upload/small", post(small_upload))
        .route("/stream", post(stream_upload).layer(DefaultBodyLimit::disable()))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
}

/// Serve the router on a free local port, and a client for it.
async fn serve() -> RpcClient {
    serve_router(router()).await.0
}

/// Serve `router` on a free local port: a client for it, and its address.
async fn serve_router(router: Router) -> (RpcClient, SocketAddr) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free local port");
    let address = listener.local_addr().expect("a bound address");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("the server runs");
    });
    (RpcClient::new(format!("http://{address}/")), address)
}

/// A client for the same server sending `x-user: user`.
fn as_user(client: &RpcClient, user: &str) -> RpcClient {
    let mut headers = header::HeaderMap::new();
    headers.insert("x-user", header::HeaderValue::from_str(user).expect("a header value"));
    let http = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("a client without TLS builds");
    RpcClient::with_http(client.base_url(), http)
}

/// A request the client would not send, or whose status it does not expose:
/// the status and the body as JSON (`null` when it is not JSON).
async fn exchange(
    client: &RpcClient,
    method: Method,
    path: &str,
    content_type: Option<&str>,
    body: &str,
) -> (u16, Value) {
    let mut request = client
        .http()
        .request(method, format!("{}{path}", client.base_url()))
        .body(body.to_string());
    if let Some(content_type) = content_type {
        request = request.header(header::CONTENT_TYPE, content_type);
    }
    let response = request.send().await.expect("the server answers");
    let status = response.status().as_u16();
    let bytes = response.bytes().await.expect("the body arrives");
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn first_code(json: &Value) -> Option<&str> {
    json.pointer("/errors/0/code").and_then(Value::as_str)
}

/// The registry code a failed call carries, or a description of what else
/// it was.
fn api_code<T: std::fmt::Debug>(result: &Called<T>) -> Result<ErrorCode, String> {
    match result {
        Err(ClientError::Api(errors)) => errors.code::<ErrorCode>().ok_or_else(|| format!("{errors:?}")),
        other => Err(format!("{other:?}")),
    }
}

// ---- inputs in the URL ----------------------------------------------------

#[tokio::test]
async fn every_kind_round_trips_through_the_query_string() {
    let client = serve().await;
    for j in json_values() {
        let input = everything(j);
        let echoed: Called<Everything> = client.call_http(&ECHO, &input).await;
        assert_eq!(echoed.ok().as_ref(), Some(&input), "{:?}", input.j);
    }

    // An absent optional and empty lists are left out, and read back as such.
    let input = Everything {
        o: Some("set".to_string()),
        tags: Vec::new(),
        ns: Vec::new(),
        js: Vec::new(),
        ..everything(json!([]))
    };
    let echoed: Called<Everything> = client.call_http(&ECHO, &input).await;
    assert_eq!(echoed.ok(), Some(input));
}

#[tokio::test]
async fn every_kind_round_trips_through_a_form_body() {
    let client = serve().await;
    for j in json_values() {
        let input = everything(j);
        let echoed: Called<Everything> = client.call_http(&ECHO_FORM, &input).await;
        assert_eq!(echoed.ok().as_ref(), Some(&input), "{:?}", input.j);
    }
}

#[tokio::test]
async fn a_form_body_wins_over_the_query_string() {
    let client = serve().await;
    let query = "/echo/form?s=query&i=1&f=1&b=false&j=1&tags=q1&tags=q2";
    let (status, json) = exchange(
        &client,
        Method::POST,
        query,
        Some("application/x-www-form-urlencoded"),
        "s=form&j=%22text%22&tags=f",
    )
    .await;
    assert_eq!(status, 200, "{json}");
    let data = json.get("data").cloned().unwrap_or_default();
    assert_eq!(data.get("s"), Some(&json!("form")), "per field: {data}");
    assert_eq!(data.get("i"), Some(&json!(1)), "a field only the query has: {data}");
    assert_eq!(data.get("j"), Some(&json!("text")));
    assert_eq!(data.get("tags"), Some(&json!(["f"])), "a list replaced whole: {data}");
}

#[tokio::test]
async fn path_parameters_carry_any_text() {
    let client = serve().await;
    for name in ["a b", "100%", "%41", "a/b", "ü😀", "a+b", "?x=1#y", "..."] {
        for j in [json!("s"), json!(7), json!({"k": "a b/c"})] {
            let input = PathInput {
                name: name.to_string(),
                n: -3,
                j: j.clone(),
                rest: Some("r&s".to_string()),
            };
            let echoed: Called<PathInput> = client.call_http(&PATHS, &input).await;
            assert_eq!(echoed.ok().as_ref(), Some(&input), "{name} {j}");
        }
    }
}

#[tokio::test]
async fn a_path_parameter_no_segment_carries_is_not_sent() {
    let client = serve().await;
    for name in ["", ".", ".."] {
        let input = json!({ "name": name, "n": 1, "j": 1 });
        let refused: Called<Value> = client.call_http(&PATHS, &input).await;
        assert!(matches!(refused, Err(ClientError::Encode(_))), "{name:?}: {refused:?}");
    }
}

#[tokio::test]
async fn a_list_path_parameter_round_trips_as_its_one_item() {
    let client = serve().await;
    let input = ListPath {
        tags: vec!["a b/c".to_string()],
        ns: vec![-7],
        js: vec![json!({"k": [1, "x"]})],
    };
    let echoed: Called<ListPath> = client.call_http(&LIST_PATHS, &input).await;
    assert_eq!(echoed.ok(), Some(input));

    // An array of any other length has no segment that spells it.
    let two = json!({ "tags": ["a", "b"], "ns": [1], "js": [1] });
    let refused: Called<Value> = client.call_http(&LIST_PATHS, &two).await;
    assert!(matches!(refused, Err(ClientError::Encode(_))), "{refused:?}");
}

#[tokio::test]
async fn a_json_body_beside_a_path_parameter() {
    let client = serve().await;
    let input = Rename {
        id: 7,
        name: "renamed".to_string(),
        padding: String::new(),
    };
    let echoed: Called<Rename> = client.call_http(&PUT_ITEM, &input).await;
    assert_eq!(echoed.ok(), Some(input));

    let invalid = json!({ "id": 7, "name": "" });
    let refused: Called<Rename> = client.call_http(&PUT_ITEM, &invalid).await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::InvalidBody));
    let pointer = match &refused {
        Err(ClientError::Api(errors)) => errors.pointer().map(str::to_string),
        _ => None,
    };
    assert_eq!(pointer.as_deref(), Some("/name"));
}

// ---- envelopes and statuses -----------------------------------------------

#[tokio::test]
async fn an_envelope_and_a_bare_value_are_each_read_as_declared() {
    let client = serve().await;
    let input = everything(json!(1));
    // `/echo` answers bare: read as the envelope, it carries neither field.
    let enveloped = HttpSpec { envelope: true, ..ECHO };
    let misread: Called<Everything> = client.call_http(&enveloped, &input).await;
    assert!(matches!(misread, Err(ClientError::Decode(_))), "{misread:?}");
    // `/echo/form` answers the envelope: read bare, it is not the value.
    let unwrapped = HttpSpec {
        envelope: false,
        ..ECHO_FORM
    };
    let misread: Called<Everything> = client.call_http(&unwrapped, &input).await;
    assert!(matches!(misread, Err(ClientError::Decode(_))), "{misread:?}");
}

#[tokio::test]
async fn an_input_the_server_cannot_read_answers_400() {
    let client = serve().await;
    let wrong = json!({ "s": "x", "i": "not a number", "f": 1, "b": true, "j": 1 });
    let refused: Called<Value> = client.call_http(&ECHO, &wrong).await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::InvalidBody));

    let (status, json) = exchange(&client, Method::GET, "/echo?s=x&i=x&f=1&b=true&j=1", None, "").await;
    assert_eq!((status, first_code(&json)), (400, Some("things::invalid")), "{json}");
    assert_eq!(json.pointer("/errors/0/source/parameter"), Some(&json!("i")));

    // A JSON-typed field sent as bare text, as a schema-blind client would.
    let (status, json) = exchange(&client, Method::GET, "/echo?s=x&i=1&f=1&b=true&j=abc", None, "").await;
    assert_eq!((status, first_code(&json)), (400, Some("things::invalid")), "{json}");

    let (status, json) = exchange(&client, Method::PUT, "/items/1", Some("application/json"), "{").await;
    assert_eq!((status, first_code(&json)), (400, Some("things::invalid")), "{json}");
}

#[tokio::test]
async fn a_missing_session_answers_401_and_a_denied_one_403() {
    let client = serve().await;
    let call = |client: RpcClient| {
        async move {
            let answer: Called<Value> = client.call_http(&GUARDED, &NoInput).await;
            answer
        }
    };
    assert_eq!(api_code(&call(client.clone()).await), Ok(ErrorCode::Unauthenticated));
    assert_eq!(api_code(&call(as_user(&client, "u1")).await), Ok(ErrorCode::Forbidden));
    let allowed = call(as_user(&client, "admin")).await;
    assert_eq!(allowed.ok(), Some(json!({ "user": "admin" })));

    let (status, _) = exchange(&client, Method::GET, "/guarded", None, "").await;
    assert_eq!(status, 401);
    let (status, json) = exchange(&as_user(&client, "u1"), Method::GET, "/guarded", None, "").await;
    assert_eq!((status, first_code(&json)), (403, Some("forbidden")));
}

#[tokio::test]
async fn a_bare_failure_answers_500_and_is_still_read_from_errors() {
    let client = serve().await;
    let failed: Called<Value> = client.call_http(&BROKEN, &NoInput).await;
    assert_eq!(api_code(&failed), Ok(ErrorCode::InternalError));
    let (status, json) = exchange(&client, Method::GET, "/broken", None, "").await;
    assert_eq!((status, first_code(&json)), (500, Some("INTERNAL_ERROR")));
}

#[tokio::test]
async fn an_rpc_call_round_trips_and_fails_through_errors() {
    let client = serve().await;
    let input = Rename {
        id: 1,
        name: "n".to_string(),
        padding: String::new(),
    };
    let echoed: Called<Rename> = client.call("/rpc/rename", &input).await;
    assert_eq!(echoed.ok(), Some(input));

    let invalid = json!({ "id": 1, "name": "" });
    let refused: Called<Rename> = client.call("/rpc/rename", &invalid).await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::InvalidBody));

    let wrong = json!({ "id": "one", "name": "n" });
    let refused: Called<Rename> = client.call("/rpc/rename", &wrong).await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::InvalidBody));
    let (status, json) = exchange(
        &client,
        Method::POST,
        "/rpc/rename",
        Some("application/json"),
        &wrong.to_string(),
    )
    .await;
    assert_eq!((status, first_code(&json)), (422, Some("things::invalid")), "{json}");
    assert_eq!(json.pointer("/errors/0/source/pointer"), Some(&json!("/id")));
}

#[tokio::test]
async fn a_json_body_over_the_limit_answers_413_with_the_envelope() {
    let client = serve().await;
    let input = Rename {
        id: 1,
        name: "n".to_string(),
        padding: "x".repeat(BODY_LIMIT),
    };
    let refused: Called<Rename> = client.call("/rpc/rename", &input).await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::InvalidBody));

    let body = serde_json::to_string(&input).expect("serializes");
    let (status, json) = exchange(&client, Method::POST, "/rpc/rename", Some("application/json"), &body).await;
    assert_eq!((status, first_code(&json)), (413, Some("things::invalid")), "{json}");
}

#[tokio::test]
async fn an_rpc_body_is_read_only_as_json() {
    let client = serve().await;
    let body = r#"{"id": 1, "name": "n"}"#;
    for content_type in [None, Some("text/plain"), Some("application/x-www-form-urlencoded")] {
        let (status, json) = exchange(&client, Method::POST, "/rpc/rename", content_type, body).await;
        assert_eq!(
            (status, first_code(&json)),
            (415, Some("things::invalid")),
            "{content_type:?}: {json}"
        );
    }
    let (status, _) = exchange(&client, Method::POST, "/rpc/rename", Some("application/json"), body).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn an_operation_without_input_still_takes_a_json_body() {
    let client = serve().await;
    let admin = as_user(&client, "admin");
    let pong: Called<String> = admin.call("/rpc/ping", &NoInput).await;
    assert_eq!(pong.ok().as_deref(), Some("pong admin"));

    // The guard first, at the envelope's status: an RPC caller reads `errors[]`.
    let refused: Called<String> = client.call("/rpc/ping", &NoInput).await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::Unauthenticated));
    let (status, _) = exchange(&client, Method::POST, "/rpc/ping", Some("application/json"), "{}").await;
    assert_eq!(status, 200);

    for (content_type, body, expected) in [
        (Some("application/json"), "", 422),
        (Some("application/json"), "not json", 422),
        (Some("text/plain"), "{}", 415),
        (None, "", 415),
    ] {
        let (status, json) = exchange(&admin, Method::POST, "/rpc/ping", content_type, body).await;
        assert_eq!(
            (status, first_code(&json)),
            (expected, Some("things::invalid")),
            "{content_type:?} {body:?}: {json}"
        );
    }
}

#[tokio::test]
async fn a_client_error_before_sending_is_encode() {
    let client = serve().await;
    let invalid_type: Called<reqwest::Response> = client
        .raw_request(
            "/convert",
            CONVERT_FIELDS,
            &json!({ "name": "a", "opts": 1 }),
            "x",
            "text/plain\r\nx-injected: 1",
        )
        .await;
    assert!(matches!(invalid_type, Err(ClientError::Encode(_))), "{invalid_type:?}");

    let unparsable: Called<Value> = RpcClient::new("not a url").call("/rpc/ping", &NoInput).await;
    assert!(matches!(unparsable, Err(ClientError::Encode(_))), "{unparsable:?}");
}

#[tokio::test]
async fn an_unreachable_server_is_transport() {
    // `https` included: without a TLS feature the client cannot connect, and
    // says so when the request is sent rather than when it is built.
    for base in ["http://127.0.0.1:1", "https://127.0.0.1:1"] {
        let failed: Called<Value> = RpcClient::new(base).call("/rpc/ping", &NoInput).await;
        assert!(matches!(failed, Err(ClientError::Transport(_))), "{base}: {failed:?}");
    }
}

// ---- subscriptions ----------------------------------------------------------

#[tokio::test]
async fn a_subscription_streams_its_events() {
    let client = serve().await;
    for label in [json!("7"), json!(["a", 1]), json!({"k": null})] {
        let input = Ticks {
            count: 3,
            fail: false,
            label: label.clone(),
        };
        let opened: Called<_> = client.subscribe::<_, Tick, _>("/ticks", TICKS_FIELDS, &input).await;
        let events: Vec<Result<Tick, StreamError>> = opened.expect("the stream opens").collect().await;
        let events: Vec<Tick> = events.into_iter().filter_map(Result::ok).collect();
        let expected: Vec<Tick> = (1..=3)
            .map(|n| {
                Tick {
                    n,
                    label: label.clone(),
                }
            })
            .collect();
        assert_eq!(events, expected, "{label}");
    }
}

#[tokio::test]
async fn a_refused_subscription_is_an_error_not_an_empty_stream() {
    let client = serve().await;
    let input = Ticks {
        count: 3,
        fail: true,
        label: json!(0),
    };
    let refused = client
        .subscribe::<_, Tick, ErrorCode>("/ticks", TICKS_FIELDS, &input)
        .await;
    assert_eq!(api_code(&refused.map(|_| ())), Ok(ErrorCode::Forbidden));

    let unreadable = json!({ "count": "many", "label": 1 });
    let refused = client
        .subscribe::<_, Tick, ErrorCode>("/ticks", TICKS_FIELDS, &unreadable)
        .await;
    assert_eq!(api_code(&refused.map(|_| ())), Ok(ErrorCode::InvalidBody));

    // A page where the stream should be is not a stream that has no events.
    let page = client.subscribe::<_, Tick, ErrorCode>("/proxy", &[], &NoInput).await;
    assert!(matches!(page, Err(ClientError::Decode(_))), "{:?}", page.map(|_| ()));
}

// ---- raw bodies -------------------------------------------------------------

async fn text(response: reqwest::Response) -> (Option<String>, String) {
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response.text().await.expect("the body arrives");
    (content_type, body)
}

#[tokio::test]
async fn a_raw_answer_is_its_bytes_whatever_their_type() {
    let client = serve().await;
    let csv: Called<reqwest::Response> = client.raw("/report", &json!({ "format": "csv" })).await;
    let csv = csv.expect("bytes");
    assert_eq!(
        csv.headers()
            .get(header::CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=\"report.csv\"")
    );
    assert_eq!(
        text(csv).await,
        (Some("text/csv".to_string()), "a,b\n1,2\n".to_string())
    );

    // JSON that is the operation's own answer, not a refusal.
    let answer: Called<reqwest::Response> = client.raw("/report", &json!({ "format": "json" })).await;
    let (content_type, body) = text(answer.expect("bytes")).await;
    assert_eq!(content_type.as_deref(), Some("application/json; charset=utf-8"));
    assert_eq!(body, r#"{"rows":[1,2]}"#);

    // Not `application/json`, so not read, whatever it holds.
    let lines: Called<reqwest::Response> = client.raw("/report", &json!({ "format": "jsonl" })).await;
    let (content_type, body) = text(lines.expect("bytes")).await;
    assert_eq!(content_type.as_deref(), Some("application/jsonl"));
    assert_eq!(body, "{\"errors\":[{\"code\":\"x\"}]}\n");

    // A page at `200` is bytes too: a raw answer has no type to check against.
    let page: Called<reqwest::Response> = client.raw("/proxy", &NoInput).await;
    assert!(page.is_ok(), "{page:?}");
}

#[tokio::test]
async fn a_raw_refusal_at_200_is_an_error() {
    let client = serve().await;
    let refused: Called<reqwest::Response> = client.raw("/report", &json!({ "format": "pdf" })).await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::NotFound));

    let refused: Called<reqwest::Response> = client.raw("/report", &json!({ "format": 1 })).await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::InvalidBody));
}

#[tokio::test]
async fn a_raw_request_sends_bytes_beside_its_query() {
    let client = serve().await;
    let input = json!({ "name": "doc 1", "opts": "7" });
    let converted: Called<reqwest::Response> = client
        .raw_request("/convert", CONVERT_FIELDS, &input, "abc", "text/plain")
        .await;
    let (content_type, body) = text(converted.expect("bytes")).await;
    assert_eq!(content_type.as_deref(), Some("text/plain"));
    assert_eq!(body, "doc 1 \"7\" ABC", "the JSON field kept its type");

    // The refusal the client used to return as a success: `200`, the envelope.
    let missing = json!({ "name": "missing", "opts": 1 });
    let refused: Called<reqwest::Response> = client
        .raw_request("/convert", CONVERT_FIELDS, &missing, "abc", "text/plain")
        .await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::NotFound));

    let unreadable = json!({ "name": "x" });
    let refused: Called<reqwest::Response> = client
        .raw_request("/convert", CONVERT_FIELDS, &unreadable, "abc", "text/plain")
        .await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::InvalidBody));
}

#[tokio::test]
async fn a_raw_request_answered_with_the_envelope() {
    let client = serve().await;
    let input = json!({ "name": "up", "opts": {"k": [1]} });
    let reported: Called<Value> = client
        .raw_request_json(
            "/upload",
            CONVERT_FIELDS,
            &input,
            vec![0u8, 1, 2],
            "application/octet-stream",
        )
        .await;
    assert_eq!(
        reported.ok(),
        Some(json!({ "name": "up", "opts": {"k": [1]}, "len": 3 }))
    );

    let missing = json!({ "name": "missing", "opts": 1 });
    let refused: Called<Value> = client
        .raw_request_json("/upload", CONVERT_FIELDS, &missing, "", "application/octet-stream")
        .await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::NotFound));
}

#[tokio::test]
async fn a_streamed_raw_request_reaches_the_service_unread() {
    let client = serve().await;
    let payload = "y".repeat(BODY_LIMIT * 4);
    let input = json!({ "name": "big", "opts": [] });
    let reported: Called<Value> = client
        .raw_request_json("/stream", CONVERT_FIELDS, &input, payload.clone(), "text/plain")
        .await;
    let expected = json!({
        "name": "big",
        "declared": payload.len(),
        "type": "text/plain",
        "read": payload.len(),
        "body": payload,
    });
    assert_eq!(
        reported.ok(),
        Some(expected),
        "past the router's limit, which the route lifts"
    );

    // A chunked body declares no length.
    let chunks = treat::rpc::futures_util::stream::iter(["ab", "cd"].map(Ok::<_, std::io::Error>));
    let reported: Called<Value> = client
        .raw_request_json(
            "/stream",
            CONVERT_FIELDS,
            &input,
            reqwest::Body::wrap_stream(chunks),
            "text/plain",
        )
        .await;
    let reported = reported.expect("an answer");
    assert_eq!(reported.get("declared"), Some(&Value::Null), "{reported}");
    assert_eq!(reported.get("body"), Some(&json!("abcd")), "{reported}");
}

/// An answer of the operation's own JSON that looks like a refusal: only the
/// `x-rpc-status` header tells them apart, and the client prefers it.
#[tokio::test]
async fn the_status_header_decides_over_the_body() {
    let client = serve().await;
    let answer: Called<reqwest::Response> = client.raw("/report", &json!({ "format": "json-errors" })).await;
    if cfg!(feature = "rpc-status-header") {
        let answer = answer.expect("`x-rpc-status: ok` says these are the bytes");
        assert_eq!(
            answer
                .headers()
                .get("x-rpc-status")
                .and_then(|value| value.to_str().ok()),
            Some("ok")
        );
        assert_eq!(text(answer).await.1, r#"{"errors":[{"code":"x"}]}"#);
    } else {
        assert!(
            matches!(answer, Err(ClientError::Api(_))),
            "without the header, the body is all there is to read: {answer:?}"
        );
    }
}

// ---- the review's findings --------------------------------------------------

#[tokio::test]
async fn an_any_route_is_sent_get_without_a_body_and_post_with_one() {
    let client = serve().await;
    let input = Rename {
        id: 3,
        name: "n".to_string(),
        padding: String::new(),
    };
    for (spec, method) in [(&ANY_QUERY, "GET"), (&ANY_FORM, "POST")] {
        let answer: Called<Value> = client.call_http(spec, &input).await;
        assert_eq!(
            answer.ok(),
            Some(json!({ "method": method, "input": { "id": 3, "name": "n", "padding": "" } })),
            "{:?}",
            spec.body
        );
    }
}

#[tokio::test]
async fn a_null_item_in_a_list_only_json_spells_is_not_sent() {
    let client = serve().await;
    let mut input = serde_json::to_value(everything(json!(1))).expect("serializes");
    input["tags"] = json!(["x", null]);
    let refused: Called<Value> = client.call_http(&ECHO, &input).await;
    assert!(matches!(refused, Err(ClientError::Encode(_))), "{refused:?}");
    let refused: Called<Value> = client.call_http(&ECHO_FORM, &input).await;
    assert!(matches!(refused, Err(ClientError::Encode(_))), "{refused:?}");

    // A `Json` list spells `null` as JSON text, and reads it back.
    let mut input = everything(json!(1));
    input.js = vec![Value::Null, json!(2)];
    let echoed: Called<Everything> = client.call_http(&ECHO, &input).await;
    assert_eq!(echoed.ok(), Some(input));
}

/// What a page on another origin posts with its visitor's cookies and without
/// a preflight: refused before the service reads a byte, buffered or streamed.
#[tokio::test]
async fn a_raw_request_from_a_cross_origin_form_is_refused_415() {
    let client = serve().await;
    for path in [
        "/convert?name=a&opts=1",
        "/upload?name=a&opts=1",
        "/stream?name=a&opts=1",
    ] {
        for content_type in [
            Some("application/x-www-form-urlencoded"),
            Some("multipart/form-data; boundary=x"),
            None,
        ] {
            let (status, json) = exchange(&client, Method::POST, path, content_type, "a=b").await;
            assert_eq!(
                (status, first_code(&json)),
                (415, Some("things::invalid")),
                "{path} {content_type:?}: {json}"
            );
            assert_eq!(json.pointer("/errors/0/source/header"), Some(&json!("content-type")));
        }
    }
    // `text/plain` is what `/convert` declares, and `/stream` takes `text/*`.
    let (status, _) = exchange(&client, Method::POST, "/upload?name=a&opts=1", Some("text/plain"), "a").await;
    assert_eq!(status, 415);

    // The client sends what it is told to; a type the operation does not
    // declare comes back as the refusal it is.
    let input = json!({ "name": "a", "opts": 1 });
    let refused: Called<Value> = client
        .raw_request_json("/upload", CONVERT_FIELDS, &input, "a", "text/plain")
        .await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::InvalidBody));
}

#[tokio::test]
async fn a_raw_body_over_its_limit_answers_413() {
    let client = serve().await;
    let input = json!({ "name": "a", "opts": 1 });
    let within: Called<Value> = client
        .raw_request_json(
            "/upload/small",
            CONVERT_FIELDS,
            &input,
            "x".repeat(SMALL_RAW_LIMIT),
            "application/octet-stream",
        )
        .await;
    assert_eq!(
        within.ok().and_then(|reported| reported.get("len").cloned()),
        Some(json!(16))
    );

    let over = "x".repeat(SMALL_RAW_LIMIT + 1);
    let refused: Called<Value> = client
        .raw_request_json(
            "/upload/small",
            CONVERT_FIELDS,
            &input,
            over.clone(),
            "application/octet-stream",
        )
        .await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::InvalidBody));
    let (status, json) = exchange(
        &client,
        Method::POST,
        "/upload/small?name=a&opts=1",
        Some("application/octet-stream"),
        &over,
    )
    .await;
    assert_eq!((status, first_code(&json)), (413, Some("things::invalid")), "{json}");
}

#[tokio::test]
async fn a_rebuilt_raw_answer_keeps_its_url_and_peer() {
    let (client, address) = serve_router(router()).await;
    let answer: Called<reqwest::Response> = client.raw("/report", &json!({ "format": "json" })).await;
    let answer = answer.expect("bytes");
    assert_eq!(answer.url().as_str(), format!("http://{address}/report"));
    assert_eq!(answer.remote_addr(), Some(address));
    assert_eq!(text(answer).await.1, r#"{"rows":[1,2]}"#);
}

/// How much of `/slow-json`'s body is sent before it waits: more than the
/// client reads to tell a refusal apart.
const SLOW_PREFIX: usize = RpcClient::RAW_PEEK_LIMIT + 4096;

/// A raw JSON answer that sends [`SLOW_PREFIX`] bytes and then waits for the
/// test to let it finish. Built by hand, without `x-rpc-status`, so the client
/// has only the body to go by, with `rpc-status-header` or without.
fn slow_json(release: Arc<tokio::sync::Notify>) -> Response {
    let head = format!(r#"{{"rows":"{}"#, "x".repeat(SLOW_PREFIX));
    let chunks = treat::rpc::futures_util::stream::unfold(Some(head), move |state| {
        let release = Arc::clone(&release);
        async move {
            match state {
                Some(head) => Some((Ok::<_, std::io::Error>(head), None)),
                None => {
                    release.notified().await;
                    Some((Ok(r#""}"#.to_string()), None))
                }
            }
        }
    })
    .take(2);
    (
        [(header::CONTENT_TYPE.as_str(), "application/json")],
        Body::from_stream(chunks),
    )
        .into_response()
}

#[tokio::test]
async fn a_large_json_answer_streams_rather_than_buffers() {
    let release = Arc::new(tokio::sync::Notify::new());
    let slow = {
        let release = Arc::clone(&release);
        post(move || std::future::ready(slow_json(Arc::clone(&release))))
    };
    let (client, address) = serve_router(Router::new().route("/slow-json", slow)).await;

    // Returned while the server still holds the end of the body back: what was
    // read to look for a refusal is handed back, not the whole answer.
    let answer = tokio::time::timeout(
        Duration::from_secs(10),
        client.raw::<_, ErrorCode>("/slow-json", &NoInput),
    )
    .await
    .expect("returned before the body ended")
    .expect("the operation's own JSON, not a refusal");
    assert_eq!(answer.status(), 200);
    assert_eq!(answer.url().as_str(), format!("http://{address}/slow-json"));
    assert_eq!(answer.remote_addr(), Some(address));
    assert_eq!(
        answer
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json")
    );

    release.notify_one();
    let body = answer.text().await.expect("the rest arrives");
    assert_eq!(body, format!(r#"{{"rows":"{}"}}"#, "x".repeat(SLOW_PREFIX)));
}

// ---- answers without a value, and refusals without an end -------------------

#[tokio::test]
async fn a_success_without_a_value_is_null_data_and_reads_back() {
    let client = serve().await;
    let input = json!({ "id": 1, "name": "n" });

    let (status, json) = exchange(
        &client,
        Method::POST,
        "/rpc/forget",
        Some("application/json"),
        &input.to_string(),
    )
    .await;
    assert_eq!((status, json), (200, json!({ "data": null })), "the premise");

    let unit: Called<()> = client.call("/rpc/forget", &input).await;
    assert!(matches!(unit, Ok(())), "{unit:?}");
    let nothing: Called<Option<Rename>> = client.call("/rpc/forget", &input).await;
    assert!(matches!(nothing, Ok(None)), "{nothing:?}");
    let value: Called<Value> = client.call("/rpc/forget", &input).await;
    assert_eq!(value.ok(), Some(Value::Null));

    // A value the operation must carry is still missing.
    let required: Called<Rename> = client.call("/rpc/forget", &input).await;
    assert!(matches!(required, Err(ClientError::Decode(_))), "{required:?}");
}

/// An answer that is not the operation's and never ends: more than the client
/// reads of a refusal, then nothing, with the connection held open.
fn endless(status: u16) -> Response {
    let head = Bytes::from(vec![b'x'; RpcClient::REFUSAL_LIMIT + 4096]);
    let chunks = treat::rpc::futures_util::stream::iter([Ok::<_, std::io::Error>(head)])
        .chain(treat::rpc::futures_util::stream::pending());
    let status = axum::http::StatusCode::from_u16(status).expect("a valid status");
    (
        status,
        [(header::CONTENT_TYPE.as_str(), "application/octet-stream")],
        Body::from_stream(chunks),
    )
        .into_response()
}

#[tokio::test]
async fn a_refusal_is_read_only_up_to_its_limit() {
    let router = Router::new()
        .route("/endless", get(|| std::future::ready(endless(200))))
        .route("/endless/502", post(|| std::future::ready(endless(502))));
    let (client, _) = serve_router(router).await;

    // A subscription answered with bytes: no event stream, and no refusal
    // within the limit, so not waited on to its end.
    let page = tokio::time::timeout(
        Duration::from_secs(10),
        client.subscribe::<_, Tick, ErrorCode>("/endless", &[], &NoInput),
    )
    .await
    .expect("returned without reading the body to its end");
    assert!(matches!(page, Err(ClientError::Decode(_))), "{:?}", page.map(|_| ()));

    let failed = tokio::time::timeout(
        Duration::from_secs(10),
        client.raw::<_, ErrorCode>("/endless/502", &NoInput),
    )
    .await
    .expect("returned without reading the body to its end");
    assert!(matches!(failed, Err(ClientError::Status(502))), "{failed:?}");
}

#[test]
fn the_refusal_limit_is_one_mebibyte() {
    assert_eq!(RpcClient::REFUSAL_LIMIT, 1 << 20);
}

/// A bare operation answered by a server that declares the failure in
/// `x-rpc-status` but leaves it at a success status: the header wins over the
/// status, so the client reads `errors[]` instead of the value.
#[tokio::test]
async fn a_bare_answer_declared_a_failure_is_read_from_errors() {
    let declared = || {
        std::future::ready(
            (
                axum::http::StatusCode::ACCEPTED,
                [
                    ("x-rpc-status", "error"),
                    (header::CONTENT_TYPE.as_str(), "application/json"),
                ],
                r#"{"errors":[{"code":"not_found"}]}"#,
            )
                .into_response(),
        )
    };
    let (client, _) = serve_router(Router::new().route("/declared", get(declared))).await;
    let spec = http(HttpMethod::Get, "/declared", &[], &[], HttpBodyKind::None, false);
    let refused: Called<Value> = client.call_http(&spec, &NoInput).await;
    assert_eq!(api_code(&refused), Ok(ErrorCode::NotFound));
}
