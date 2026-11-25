//! A generated handler's steps through a real axum router: `http_input` and
//! its statuses, `json_input` / `no_input`, the context guards, and
//! `respond` / `respond_bare` / `respond_http`.
#![cfg(all(feature = "rpc-server", feature = "derive"))]

mod rpc_server;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Request};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::Response;
use axum::routing::{any, get, post, put};
use rpc_server::{ErrorCode, FIND_FIELDS, INVALID, Rt, code, read, unset_or};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower::ServiceExt;
use treat::rpc::server::{Ctx, QueryField, QueryKind, Refusal, ResolveContext};
use treat::rpc::{HttpBodyKind, HttpMethod, HttpSpec};
use treat::{ApiError, ApiResponse};
use validator::Validate;

type Handled = Result<Response, Refusal>;

#[derive(Debug, Serialize, Deserialize, Validate)]
struct Find {
    #[validate(email(message = "not an email"))]
    email: String,
    limit: Option<i64>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Validate)]
struct Info {
    session_id: Option<String>,
    #[serde(default)]
    verbose: bool,
    #[serde(default)]
    tags: Vec<String>,
}

static INFO_FIELDS: &[QueryField] = &[
    QueryField {
        name: "session_id",
        kind: QueryKind::Str,
        list: false,
    },
    QueryField {
        name: "verbose",
        kind: QueryKind::Bool,
        list: false,
    },
    QueryField {
        name: "tags",
        kind: QueryKind::Str,
        list: true,
    },
];

#[derive(Debug, Serialize, Deserialize, Validate)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
struct Rename {
    id: i64,
    #[validate(length(min = 1, message = "empty"))]
    name: String,
}

static RENAME_FIELDS: &[QueryField] = &[
    QueryField {
        name: "id",
        kind: QueryKind::Int,
        list: false,
    },
    QueryField {
        name: "name",
        kind: QueryKind::Str,
        list: false,
    },
];

#[derive(Debug, Serialize, Deserialize, Validate)]
struct UserInput {
    user_id: String,
}

static USER_FIELDS: &[QueryField] = &[QueryField {
    name: "user_id",
    kind: QueryKind::Str,
    list: false,
}];

/// A generated server's description of an `@http` operation: one `static`
/// each, shared with its client.
const fn spec(
    method: HttpMethod,
    path: &'static str,
    fields: &'static [QueryField],
    path_fields: &'static [&'static str],
    body: HttpBodyKind,
) -> HttpSpec {
    HttpSpec {
        method,
        path,
        fields,
        path_fields,
        body,
        envelope: false,
    }
}

static FIND: HttpSpec = spec(HttpMethod::Get, "/find", FIND_FIELDS, &[], HttpBodyKind::None);
static RAW_FIND: HttpSpec = spec(HttpMethod::Get, "/raw/find", FIND_FIELDS, &[], HttpBodyKind::None);
static INFO: HttpSpec = spec(HttpMethod::Any, "/info", INFO_FIELDS, &[], HttpBodyKind::Form);
static RENAME: HttpSpec = spec(
    HttpMethod::Put,
    "/items/{id}",
    RENAME_FIELDS,
    &["id"],
    HttpBodyKind::Json,
);
static USER: HttpSpec = spec(
    HttpMethod::Get,
    "/user/{user_id}",
    USER_FIELDS,
    &["user_id"],
    HttpBodyKind::None,
);

/// A path parameter the field table leaves out, under each body kind: read as
/// a string by all three.
#[derive(Debug, Serialize, Deserialize, Validate)]
struct Tagged {
    tag: String,
    #[serde(default)]
    note: Option<String>,
}

static NOTE_FIELDS: &[QueryField] = &[QueryField {
    name: "note",
    kind: QueryKind::Str,
    list: false,
}];

static TAGGED_QUERY: HttpSpec = spec(
    HttpMethod::Get,
    "/tagged/{tag}",
    NOTE_FIELDS,
    &["tag"],
    HttpBodyKind::None,
);
static TAGGED_FORM: HttpSpec = spec(
    HttpMethod::Post,
    "/tagged/{tag}",
    NOTE_FIELDS,
    &["tag"],
    HttpBodyKind::Form,
);
static TAGGED_JSON: HttpSpec = spec(
    HttpMethod::Put,
    "/tagged/{tag}",
    NOTE_FIELDS,
    &["tag"],
    HttpBodyKind::Json,
);

/// Fields serde types more narrowly than their query kinds: integers that must
/// fit a `u8`, one of them in the path. Described with the `const`
/// constructors a generated `static` uses.
#[derive(Debug, Serialize, Deserialize, Validate)]
struct Levels {
    slot: u8,
    level: u8,
}

static LEVELS_FIELDS: &[QueryField] = &[QueryField::int("slot"), QueryField::int("level")];
static LEVELS_FORM: HttpSpec = HttpSpec::post("/levels/{slot}")
    .fields(LEVELS_FIELDS)
    .path_fields(&["slot"])
    .body(HttpBodyKind::Form)
    .bare();
static LEVELS_JSON: HttpSpec = HttpSpec::put("/levels/{slot}")
    .fields(LEVELS_FIELDS)
    .path_fields(&["slot"])
    .bare();

/// A context resolved from an `x-user` header; `x-store: down` stands for a
/// session store that cannot be reached.
#[derive(Debug)]
struct Session {
    user: String,
}

impl ResolveContext for Session {
    async fn from_parts(parts: &mut Parts) -> Result<Self, Refusal> {
        let header = |name| parts.headers.get(name).and_then(|value| value.to_str().ok());
        if header("x-store") == Some("down") {
            let unavailable = ApiError::from(ErrorCode::InternalError).with_status(503);
            return Err(unavailable.into());
        }
        match header("x-user") {
            Some(user) => Ok(Self { user: user.to_string() }),
            None => Err(ApiError::from(ErrorCode::Unauthenticated).into()),
        }
    }
}

impl Session {
    /// What a generated `require_role` does: the policy is the project's.
    fn require_admin(&self) -> Result<(), Refusal> {
        match self.user == "admin" {
            true => Ok(()),
            false => Err(ApiError::from(ErrorCode::NotFound).into()),
        }
    }
}

fn echo<T: Serialize>(input: T) -> Response {
    Rt::respond_bare(Ok::<_, ApiError<ErrorCode>>(input))
}

/// A generated `@http` handler: the input as the spec says, the answer as it
/// says.
async fn find(request: Request) -> Handled {
    let input: Find = Rt::http_input(request, &FIND).await?;
    Ok(Rt::respond_http(&FIND, Ok::<_, ApiError<ErrorCode>>(input)))
}

async fn info(request: Request) -> Handled {
    let input: Info = Rt::http_input(request, &INFO).await?;
    Ok(echo(input))
}

async fn rename(request: Request) -> Handled {
    let input: Rename = Rt::http_input(request, &RENAME).await?;
    Ok(echo(input))
}

async fn user(request: Request) -> Handled {
    let input: UserInput = Rt::http_input(request, &USER).await?;
    Ok(echo(input))
}

async fn raw_find(request: Request) -> Handled {
    let input: Find = Rt::read_http_input(request, &RAW_FIND).await?;
    Ok(echo(input))
}

async fn tagged_query(request: Request) -> Handled {
    Ok(echo(Rt::http_input::<Tagged>(request, &TAGGED_QUERY).await?))
}

async fn tagged_form(request: Request) -> Handled {
    Ok(echo(Rt::http_input::<Tagged>(request, &TAGGED_FORM).await?))
}

async fn tagged_json(request: Request) -> Handled {
    Ok(echo(Rt::http_input::<Tagged>(request, &TAGGED_JSON).await?))
}

async fn levels_form(request: Request) -> Handled {
    Ok(echo(Rt::http_input::<Levels>(request, &LEVELS_FORM).await?))
}

async fn levels_json(request: Request) -> Handled {
    Ok(echo(Rt::http_input::<Levels>(request, &LEVELS_JSON).await?))
}

/// An `@http` operation guarded by a session and a role.
async fn whoami(ctx: Result<Ctx<Session>, Refusal>) -> Handled {
    let ctx = Rt::http_session(ctx)?;
    Rt::http_access(ctx.require_admin())?;
    Ok(echo(json!({ "user": ctx.user })))
}

/// An RPC operation guarded the same way: refusals keep the envelope's status.
async fn rpc_whoami(Ctx(ctx): Ctx<Session>, request: Request) -> Handled {
    ctx.require_admin()?;
    Rt::no_input(request).await?;
    Ok(Rt::respond(Ok::<_, ApiError<ErrorCode>>(treat::success(
        json!({ "user": ctx.user }),
    ))))
}

/// A plain RPC operation with an input.
async fn rpc_rename(request: Request) -> Handled {
    let input: Rename = Rt::json_input(request).await?;
    Ok(Rt::respond(Ok::<_, ApiError<ErrorCode>>(treat::success(input))))
}

fn router() -> Router {
    Router::new()
        .route("/find", get(find))
        .route("/raw/find", get(raw_find))
        .route("/info", any(info))
        .route("/items/{id}", put(rename))
        .route("/user/{user_id}", get(user))
        .route("/tagged/{tag}", get(tagged_query).post(tagged_form).put(tagged_json))
        .route("/whoami", get(whoami))
        .route("/rpc/whoami", post(rpc_whoami))
        .route("/rpc/rename", post(rpc_rename))
        .route("/levels/{slot}", post(levels_form).put(levels_json))
        .layer(DefaultBodyLimit::max(64))
}

async fn call(
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: impl Into<axum::body::Body>,
) -> (u16, serde_json::Value) {
    let mut request = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = request.body(body.into()).expect("a valid request");
    let response = router().oneshot(request).await.expect("the router is infallible");
    read(response).await
}

const FORM: (&str, &str) = ("content-type", "application/x-www-form-urlencoded");

#[tokio::test]
async fn the_query_string_is_the_input() {
    let (status, json) = call("GET", "/find?email=a%40b.c&limit=3&tags=x&tags=y", &[], "").await;
    assert_eq!(status, 200);
    assert_eq!(json, json!({ "email": "a@b.c", "limit": 3, "tags": ["x", "y"] }));
}

#[tokio::test]
async fn an_input_that_does_not_read_answers_400() {
    let (status, json) = call("GET", "/find?email=a%40b.c&limit=three", &[], "").await;
    assert_eq!(status, 400);
    assert_eq!(code(&json), Some(INVALID), "{json}");
    assert_eq!(
        json.pointer("/errors/0/source/parameter"),
        Some(&json!("limit")),
        "{json}"
    );

    let (status, json) = call("GET", "/find?limit=1", &[], "").await;
    assert_eq!(status, 400, "a missing field: {json}");
    assert_eq!(code(&json), Some(INVALID));

    let (status, json) = call("GET", "/find?email=caf%E9", &[], "").await;
    assert_eq!(status, 400, "not UTF-8: {json}");
    assert_eq!(code(&json), Some(INVALID));
}

#[tokio::test]
async fn an_input_that_breaks_a_rule_answers_400() {
    let (status, json) = call("GET", "/find?email=nope", &[], "").await;
    assert_eq!(status, 400);
    assert_eq!(code(&json), Some(INVALID), "{json}");
    assert_eq!(json.pointer("/errors/0/message"), Some(&json!("not an email")));
    assert_eq!(json.pointer("/errors/0/source/pointer"), Some(&json!("/email")));
}

#[tokio::test]
async fn the_path_wins_over_the_query_string() {
    let (status, json) = call("GET", "/user/u%201?user_id=evil", &[], "").await;
    assert_eq!(status, 200);
    assert_eq!(json, json!({ "user_id": "u 1" }));
}

#[tokio::test]
async fn a_json_body_beside_the_path() {
    let body = r#"{"id": 666, "name": "renamed"}"#;
    let (status, json) = call("PUT", "/items/7", &[("content-type", "application/json")], body).await;
    assert_eq!(status, 200);
    assert_eq!(
        json,
        json!({ "id": 7, "name": "renamed" }),
        "the path's id wins, typed as its field"
    );

    let (status, json) = call("PUT", "/items/x", &[], r#"{"name": "n"}"#).await;
    assert_eq!(status, 400, "a path value is typed like a query one: {json}");
    assert_eq!(json.pointer("/errors/0/source/parameter"), Some(&json!("id")));

    let (status, json) = call("PUT", "/items/7", &[], r#"{"name": ""}"#).await;
    assert_eq!(status, 400, "a rule broken: {json}");
    assert_eq!(json.pointer("/errors/0/source/pointer"), Some(&json!("/name")));

    // A body that is no JSON object, or no JSON: the registry's code at `400`,
    // the status an `@http` caller reads for any input it got wrong.
    for body in ["[1]", "{", ""] {
        let (status, json) = call("PUT", "/items/7", &[], body).await;
        assert_eq!((status, code(&json)), (400, Some(INVALID)), "{body}: {json}");
    }

    let (status, json) = call("PUT", "/items/7", &[], r#"{"name": 5}"#).await;
    assert_eq!((status, code(&json)), (400, Some(INVALID)), "{json}");
    assert_eq!(
        json.pointer("/errors/0/source/pointer"),
        Some(&json!("/name")),
        "a field of the wrong type is located"
    );

    let large = format!(r#"{{"name": "{}"}}"#, "x".repeat(100));
    let (status, json) = call("PUT", "/items/7", &[], large).await;
    assert_eq!(
        (status, code(&json)),
        (413, Some(INVALID)),
        "over the body limit: the envelope at the limit's status: {json}"
    );
}

#[tokio::test]
async fn a_form_body_over_the_query_string() {
    let (status, json) = call("GET", "/info?session_id=q", &[], "").await;
    assert_eq!(status, 200);
    assert_eq!(json, json!({ "session_id": "q", "verbose": false, "tags": [] }));

    let (_, json) = call(
        "POST",
        "/info?session_id=q&verbose=false",
        &[FORM],
        "session_id=f&verbose=true",
    )
    .await;
    assert_eq!(
        json,
        json!({ "session_id": "f", "verbose": true, "tags": [] }),
        "the body wins"
    );

    let (_, json) = call("POST", "/info?session_id=q", &[FORM], "verbose=true").await;
    assert_eq!(
        json,
        json!({ "session_id": "q", "verbose": true, "tags": [] }),
        "per field"
    );

    let (_, json) = call("PATCH", "/info?tags=a&tags=b&verbose=true", &[FORM], "tags=c").await;
    assert_eq!(
        json,
        json!({ "session_id": null, "verbose": true, "tags": ["c"] }),
        "a list is replaced whole"
    );
}

#[tokio::test]
async fn a_form_is_known_by_its_content_type_in_any_case() {
    let cased = ("Content-Type", "Application/X-WWW-Form-Urlencoded; charset=UTF-8");
    let (_, json) = call("POST", "/info?session_id=q", &[cased], "session_id=c").await;
    assert_eq!(json.pointer("/session_id"), Some(&json!("c")));

    let other = ("content-type", "application/json");
    let (status, json) = call("POST", "/info?session_id=q", &[other], r#"{"session_id":"j"}"#).await;
    assert_eq!(status, 200, "a body that is not a form is not read: {json}");
    assert_eq!(json.pointer("/session_id"), Some(&json!("q")));

    let (_, json) = call("POST", "/info?session_id=q", &[], "session_id=none").await;
    assert_eq!(
        json.pointer("/session_id"),
        Some(&json!("q")),
        "nor is one with no content type"
    );

    // The media type is compared whole, not as a prefix.
    for near in [
        "application/x-www-form-urlencodedx",
        "application/x-www-form-urlencoded-v2; charset=utf-8",
    ] {
        let (status, json) = call("POST", "/info?session_id=q", &[("content-type", near)], "session_id=n").await;
        assert_eq!(
            (status, json.pointer("/session_id")),
            (200, Some(&json!("q"))),
            "{near}: not a form, not read"
        );
    }
}

#[tokio::test]
async fn a_path_parameter_the_table_leaves_out_is_a_string_under_every_body() {
    let (status, json) = call("GET", "/tagged/7?note=q", &[], "").await;
    assert_eq!((status, json), (200, json!({ "tag": "7", "note": "q" })), "query");

    let (status, json) = call("POST", "/tagged/7", &[FORM], "note=f&tag=evil").await;
    assert_eq!((status, json), (200, json!({ "tag": "7", "note": "f" })), "form");

    let (status, json) = call("PUT", "/tagged/7", &[], r#"{"note": "j", "tag": "evil"}"#).await;
    assert_eq!((status, json), (200, json!({ "tag": "7", "note": "j" })), "JSON");
}

#[tokio::test]
async fn a_form_that_cannot_be_read_keeps_its_status() {
    let large = format!("session_id={}", "x".repeat(100));
    let (status, json) = call("POST", "/info", &[FORM], large).await;
    assert_eq!(status, 413, "{json}");
    assert_eq!(code(&json), Some(INVALID), "{json}");
    assert_eq!(
        json.pointer("/errors/0/message"),
        Some(&json!("the form body cannot be read"))
    );

    let (status, json) = call("POST", "/info", &[FORM], &b"session_id=caf\xe9"[..]).await;
    assert_eq!(status, 400, "a raw Latin-1 byte: {json}");
    assert_eq!(code(&json), Some(INVALID));
}

#[tokio::test]
async fn session_and_access_refusals_carry_a_status() {
    let (status, json) = call("GET", "/whoami", &[], "").await;
    assert_eq!(status, unset_or(401));
    assert_eq!(code(&json), Some("unauthenticated"));

    let (status, json) = call("GET", "/whoami", &[("x-store", "down")], "").await;
    assert_eq!(status, 503, "a refusal with its own status keeps it: {json}");

    let (status, json) = call("GET", "/whoami", &[("x-user", "u1")], "").await;
    assert_eq!(status, unset_or(403));
    assert_eq!(code(&json), Some("not_found"));

    let (status, json) = call("GET", "/whoami", &[("x-user", "admin")], "").await;
    assert_eq!((status, json), (200, json!({ "user": "admin" })));
}

const JSON: (&str, &str) = ("content-type", "application/json");

#[tokio::test]
async fn an_rpc_handler_answers_refusals_with_the_envelope_s_status() {
    let (status, json) = call("POST", "/rpc/whoami", &[JSON], "{}").await;
    let envelope = treat::DEFAULT_ERROR_STATUS;
    assert_eq!((status, code(&json)), (envelope, Some("unauthenticated")));

    let (status, json) = call("POST", "/rpc/whoami", &[JSON, ("x-user", "u1")], "{}").await;
    assert_eq!(
        (status, code(&json)),
        (envelope, Some("not_found")),
        "the guard runs before the body is read"
    );

    let (status, json) = call("POST", "/rpc/whoami", &[JSON, ("x-user", "admin")], "{}").await;
    assert_eq!((status, json), (200, json!({ "data": { "user": "admin" } })));

    for body in ["", "nope", "{"] {
        let (status, json) = call("POST", "/rpc/whoami", &[JSON, ("x-user", "admin")], body).await;
        assert_eq!(
            (status, code(&json)),
            (422, Some(INVALID)),
            "{body:?}: no input still means a JSON body"
        );
    }

    let (status, json) = call("POST", "/rpc/whoami", &[("x-user", "admin")], "{}").await;
    assert_eq!(
        (status, code(&json)),
        (415, Some(INVALID)),
        "nor a body without a JSON content type: {json}"
    );
}

#[tokio::test]
async fn json_input_reads_and_validates_the_body() {
    let (status, json) = call("POST", "/rpc/rename", &[JSON], r#"{"id": 1, "name": "n"}"#).await;
    assert_eq!((status, json), (200, json!({ "data": { "id": 1, "name": "n" } })));

    let (status, json) = call("POST", "/rpc/rename", &[JSON], r#"{"id": 1, "name": ""}"#).await;
    assert_eq!(status, 200, "an RPC refusal is read from `errors[]`");
    assert_eq!(code(&json), Some(INVALID), "{json}");
    assert_eq!(json.pointer("/errors/0/source/pointer"), Some(&json!("/name")));

    let (status, json) = call("POST", "/rpc/rename", &[JSON], r#"{"id": "x"}"#).await;
    assert_eq!(
        (status, code(&json)),
        (422, Some(INVALID)),
        "the registry's code at `ApiJson`'s status: {json}"
    );
    assert_eq!(json.pointer("/errors/0/source/pointer"), Some(&json!("/id")));

    let large = format!(r#"{{"id": 1, "name": "{}"}}"#, "x".repeat(100));
    let (status, json) = call("POST", "/rpc/rename", &[JSON], large).await;
    assert_eq!((status, code(&json)), (413, Some(INVALID)), "over the limit: {json}");
}

#[tokio::test]
async fn an_rpc_body_must_be_declared_json() {
    let body = r#"{"id": 1, "name": "n"}"#;
    for content_type in [
        "application/json",
        "Application/JSON; charset=utf-8",
        "application/vnd.api+json",
        "application/problem+JSON",
    ] {
        let (status, json) = call("POST", "/rpc/rename", &[("content-type", content_type)], body).await;
        assert_eq!(status, 200, "{content_type}: {json}");
    }
    for content_type in [
        None,
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
        Some("multipart/form-data; boundary=x"),
        Some("application/jsonl"),
        Some("json"),
        Some("+json"),
    ] {
        let headers: Vec<(&str, &str)> = content_type.map(|value| ("content-type", value)).into_iter().collect();
        let (status, json) = call("POST", "/rpc/rename", &headers, body).await;
        assert_eq!((status, code(&json)), (415, Some(INVALID)), "{content_type:?}: {json}");
        assert_eq!(
            json.pointer("/errors/0/source/header"),
            Some(&json!("content-type")),
            "{json}"
        );
    }
}

#[tokio::test]
async fn respond_keeps_the_envelope_and_the_failure_s_status() {
    let ok: Result<ApiResponse<i32>, ApiError<ErrorCode>> = Ok(treat::success(1));
    assert_eq!(read(Rt::respond(ok)).await, (200, json!({ "data": 1 })));

    let failed: Result<ApiResponse<i32>, _> = Err(ApiError::from(ErrorCode::NotFound));
    let (status, json) = read(Rt::respond(failed)).await;
    assert_eq!((status, code(&json)), (treat::DEFAULT_ERROR_STATUS, Some("not_found")));

    let failed: Result<ApiResponse<i32>, _> = Err(ApiError::from(ErrorCode::NotFound).with_status(404));
    assert_eq!(read(Rt::respond(failed)).await.0, 404);
}

#[tokio::test]
async fn respond_bare_writes_the_value_and_a_failure_at_500() {
    let ok: Result<i32, ApiError<ErrorCode>> = Ok(1);
    assert_eq!(read(Rt::respond_bare(ok)).await, (200, json!(1)));

    let failed: Result<i32, _> = Err(ApiError::from(ErrorCode::NotFound));
    let (status, json) = read(Rt::respond_bare(failed)).await;
    assert_eq!(
        (status, code(&json)),
        (500, Some("not_found")),
        "a bare failure cannot stay at 200"
    );

    let failed: Result<i32, _> = Err(ApiError::from(ErrorCode::NotFound).with_status(404));
    assert_eq!(
        read(Rt::respond_bare(failed)).await.0,
        404,
        "a status of its own is kept"
    );
}

#[tokio::test]
async fn respond_http_answers_as_the_spec_says() {
    let bare = spec(HttpMethod::Get, "/x", &[], &[], HttpBodyKind::None);
    let enveloped = HttpSpec { envelope: true, ..bare };

    let ok: Result<i32, ApiError<ErrorCode>> = Ok(1);
    assert_eq!(read(Rt::respond_http(&bare, ok)).await, (200, json!(1)));
    let failed: Result<i32, _> = Err(ApiError::from(ErrorCode::NotFound));
    let (status, json) = read(Rt::respond_http(&bare, failed)).await;
    assert_eq!(
        (status, code(&json)),
        (500, Some("not_found")),
        "bare: never a 200 failure"
    );

    let ok: Result<ApiResponse<i32>, ApiError<ErrorCode>> = Ok(treat::success(1));
    assert_eq!(
        read(Rt::respond_http(&enveloped, ok)).await,
        (200, json!({ "data": 1 }))
    );
    let failed: Result<ApiResponse<i32>, _> = Err(ApiError::from(ErrorCode::NotFound));
    let (status, json) = read(Rt::respond_http(&enveloped, failed)).await;
    assert_eq!(
        (status, code(&json)),
        (treat::DEFAULT_ERROR_STATUS, Some("not_found")),
        "enveloped: the failure's own status, as an RPC answer"
    );
    let failed: Result<ApiResponse<i32>, _> = Err(ApiError::from(ErrorCode::NotFound).with_status(404));
    assert_eq!(read(Rt::respond_http(&enveloped, failed)).await.0, 404);
}

/// With `rpc-status-header`, a bare success says `ok` as an envelope would,
/// and an enveloped one says what its `errors[]` says.
#[cfg(feature = "rpc-status-header")]
#[tokio::test]
async fn every_http_answer_carries_its_outcome() {
    let outcome = |response: &Response| {
        response
            .headers()
            .get("x-rpc-status")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    };
    let bare = spec(HttpMethod::Get, "/x", &[], &[], HttpBodyKind::None);
    let enveloped = HttpSpec { envelope: true, ..bare };

    let response = Rt::respond_bare(Ok::<_, ApiError<ErrorCode>>(1));
    assert_eq!(outcome(&response).as_deref(), Some("ok"));
    let response = Rt::respond_http(&bare, Ok::<_, ApiError<ErrorCode>>(1));
    assert_eq!(outcome(&response).as_deref(), Some("ok"));
    let response = Rt::respond_http(&bare, Err::<i32, _>(ApiError::from(ErrorCode::NotFound)));
    assert_eq!(outcome(&response).as_deref(), Some("error"));

    let ok: Result<ApiResponse<i32>, ApiError<ErrorCode>> = Ok(treat::success(1));
    assert_eq!(outcome(&Rt::respond_http(&enveloped, ok)).as_deref(), Some("ok"));
    let carried: Result<ApiResponse<i32>, ApiError<ErrorCode>> =
        Ok(treat::failure([treat::error(ErrorCode::NotFound).into_error_message()]));
    let response = Rt::respond_http(&enveloped, carried);
    assert_eq!(
        outcome(&response).as_deref(),
        Some("error"),
        "an envelope carrying errors, as `ApiResponse` marks it"
    );
}

#[tokio::test]
async fn read_http_input_leaves_the_status_and_the_rules_alone() {
    let (status, json) = call("GET", "/raw/find?email=nope", &[], "").await;
    assert_eq!(
        (status, json.pointer("/email")),
        (200, Some(&json!("nope"))),
        "no validation"
    );

    let (status, json) = call("GET", "/raw/find", &[], "").await;
    assert_eq!(
        (status, code(&json)),
        (200, Some(INVALID)),
        "the envelope's status; `http_input` gives it 400"
    );
}

#[tokio::test]
async fn a_path_parameter_that_is_not_utf8_answers_400() {
    let (status, json) = call("GET", "/user/caf%E9", &[], "").await;
    assert_eq!((status, code(&json)), (400, Some(INVALID)), "{json}");
}

#[tokio::test]
async fn a_value_serde_refuses_names_its_parameter_wherever_it_travelled() {
    let cases = [
        ("/levels/1?level=300", "", "level", "the query string"),
        ("/levels/1", "level=300", "level", "the form"),
        ("/levels/300?level=1", "", "slot", "the path"),
    ];
    for (uri, form, parameter, place) in cases {
        let (status, json) = call("POST", uri, &[FORM], form).await;
        assert_eq!((status, code(&json)), (400, Some(INVALID)), "{place}: {json}");
        assert_eq!(
            json.pointer("/errors/0/source/parameter"),
            Some(&json!(parameter)),
            "{place}: {json}"
        );
    }

    let (status, json) = call("POST", "/levels/2", &[FORM], "level=3").await;
    assert_eq!((status, json), (200, json!({ "slot": 2, "level": 3 })));

    // A JSON body keeps its pointer: there the caller wrote a document.
    let (status, json) = call("PUT", "/levels/1", &[], r#"{"level": 300}"#).await;
    assert_eq!((status, code(&json)), (400, Some(INVALID)), "{json}");
    assert_eq!(
        json.pointer("/errors/0/source/pointer"),
        Some(&json!("/level")),
        "{json}"
    );
    assert_eq!(json.pointer("/errors/0/source/parameter"), None, "{json}");
}

/// A value whose `Serialize` fails, as a map with non-string keys does.
struct Unserializable;

impl Serialize for Unserializable {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("not serializable"))
    }
}

/// A bare success that cannot be written is axum's `500`, and with
/// `rpc-status-header` not marked `ok`: the header must not call it a success.
#[tokio::test]
async fn a_bare_success_that_cannot_be_written_is_not_marked_ok() {
    let response = Rt::respond_bare(Ok::<_, ApiError<ErrorCode>>(Unserializable));
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.headers().get("x-rpc-status"), None);

    let bare = spec(HttpMethod::Get, "/x", &[], &[], HttpBodyKind::None);
    let response = Rt::respond_http(&bare, Ok::<_, ApiError<ErrorCode>>(Unserializable));
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.headers().get("x-rpc-status"), None);
}

/// A registry serde renames and `#[code]` does not: its wire name is serde's
/// (`snake_case`), not the identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, treat::ApiErrorCode, treat::rpc::WireEnum)]
#[serde(rename_all = "snake_case")]
enum SnakeCode {
    InvalidBody,
    InternalError,
}

impl treat::rpc::RuntimeCodes for SnakeCode {
    const INVALID_BODY: Self = Self::InvalidBody;
    const INTERNAL_ERROR: Self = Self::InternalError;
}

type SnakeRt = treat::rpc::Runtime<SnakeCode>;

async fn snake_rpc_rename(request: Request) -> Handled {
    let input: Rename = SnakeRt::json_input(request).await?;
    Ok(SnakeRt::respond(Ok::<_, ApiError<SnakeCode>>(treat::success(input))))
}

async fn snake_http_rename(request: Request) -> Handled {
    let input: Rename = SnakeRt::http_input(request, &RENAME).await?;
    Ok(SnakeRt::respond_bare(Ok::<_, ApiError<SnakeCode>>(input)))
}

/// Every refusal the runtime builds spells its code by wire name, whichever
/// step built it: by `ErrorMessage` (a rule broken, a query unread) or by
/// `ApiError` (a body of the wrong type, unparsable or too large).
#[tokio::test]
async fn every_runtime_refusal_spells_the_code_by_its_wire_name() {
    // `Display` spells serde's name too, so a handler's `ApiError` and the
    // runtime's own refusals carry one code.
    assert_eq!(SnakeCode::InvalidBody.to_string(), "invalid_body");
    let (_, json) = read(ApiError::from(SnakeCode::InvalidBody)).await;
    assert_eq!(code(&json), Some("invalid_body"), "a handler's failure: {json}");

    let large = format!(r#"{{"id": 1, "name": "{}"}}"#, "x".repeat(100));
    let cases = [
        ("POST /rpc/rename", "application/json", r#"{"id": 1, "name": ""}"#, 200),
        ("POST /rpc/rename", "application/json", r#"{"id": "x"}"#, 422),
        ("POST /rpc/rename", "application/json", "{", 422),
        ("POST /rpc/rename", "text/plain", "{}", 415),
        ("POST /rpc/rename", "application/json", large.as_str(), 413),
        ("PUT /items/x", "application/json", r#"{"name": "n"}"#, 400),
        ("PUT /items/1", "application/json", r#"{"name": 1}"#, 400),
    ];
    let router = Router::new()
        .route("/rpc/rename", post(snake_rpc_rename))
        .route("/items/{id}", put(snake_http_rename))
        .layer(DefaultBodyLimit::max(64));
    for (route, content_type, body, expected) in cases {
        let (method, uri) = route.split_once(' ').expect("a method and a path");
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", content_type)
            .body(axum::body::Body::from(body.to_string()))
            .expect("a valid request");
        let response = router.clone().oneshot(request).await.expect("the router is infallible");
        let (status, json) = read(response).await;
        let got = (status, code(&json));
        assert_eq!(got, (expected, Some("invalid_body")), "{route} {body}: {json}");
    }

    let (_, json) = read(SnakeRt::internal_failure("boom")).await;
    assert_eq!(code(&json), Some("internal_error"));
}

#[derive(Debug, Serialize, Deserialize, Validate)]
struct Optional {
    slot: Option<i64>,
    name: Option<String>,
}

static SLOT_FIELDS: &[QueryField] = &[QueryField::int("slot"), QueryField::str("name")];

/// `@http(method: ANY, path: "/maybe/{slot}", body: JSON)`
static ANY_JSON: HttpSpec = spec(
    HttpMethod::Any,
    "/maybe/{slot}",
    SLOT_FIELDS,
    &["slot"],
    HttpBodyKind::Json,
);
/// `@http(method: PUT, path: "/maybe/{slot}", body: JSON)`
static PUT_JSON: HttpSpec = HttpSpec {
    method: HttpMethod::Put,
    ..ANY_JSON
};

async fn any_json(request: Request) -> Handled {
    Ok(echo(Rt::http_input::<Optional>(request, &ANY_JSON).await?))
}

async fn put_json(request: Request) -> Handled {
    Ok(echo(Rt::http_input::<Optional>(request, &PUT_JSON).await?))
}

/// An `ANY` route reads a request without a body from its query string; a
/// route bound to one method still needs its JSON body.
#[tokio::test]
async fn an_any_route_without_a_json_body_reads_the_query_string() {
    let router = Router::new().route("/maybe/{slot}", any(any_json));
    let send = |method: &'static str, body: &'static str| send_to(&router, method, "/maybe/3", body);
    let (status, json) = send_to(&router, "GET", "/maybe/3?name=q&slot=9", "").await;
    assert_eq!(
        (status, json),
        (200, json!({ "slot": 3, "name": "q" })),
        "the path wins"
    );
    let (status, json) = send_to(&router, "GET", "/maybe/x", "").await;
    assert_eq!((status, code(&json)), (400, Some(INVALID)), "{json}");
    assert_eq!(send("GET", "").await, (200, json!({ "slot": 3, "name": null })));
    assert_eq!(send("DELETE", "").await, (200, json!({ "slot": 3, "name": null })));
    let (status, json) = send("POST", r#"{"name": "n"}"#).await;
    assert_eq!((status, json), (200, json!({ "slot": 3, "name": "n" })));
    let (status, json) = send("POST", "{").await;
    assert_eq!(
        (status, code(&json)),
        (400, Some(INVALID)),
        "a body is still parsed: {json}"
    );

    let router = Router::new().route("/maybe/{slot}", put(put_json));
    let (status, json) = send_to(&router, "PUT", "/maybe/3?name=q", "").await;
    assert_eq!(
        (status, code(&json)),
        (400, Some(INVALID)),
        "PUT needs its body: {json}"
    );
}

async fn send_to(router: &Router, method: &str, uri: &str, body: &'static str) -> (u16, serde_json::Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .body(axum::body::Body::from(body))
        .expect("a valid request");
    read(router.clone().oneshot(request).await.expect("the router is infallible")).await
}

/// A handler whose operation reads no path parameter runs outside a matched
/// route too: as a service of its own, there are none for axum to give.
#[tokio::test]
async fn a_handler_without_path_fields_does_not_read_the_path() {
    use axum::handler::HandlerWithoutStateExt;
    let request = Request::builder()
        .uri("/find?email=a%40b.c")
        .body(axum::body::Body::empty())
        .expect("a valid request");
    let response = find.into_service().oneshot(request).await.expect("infallible");
    let (status, json) = read(response).await;
    assert_eq!(
        (status, json),
        (200, json!({ "email": "a@b.c", "limit": null, "tags": [] }))
    );
}

/// A bare failure at any `2xx` cannot be told from a success, so it answers
/// `500`; a status outside `2xx` is kept.
#[tokio::test]
async fn a_bare_failure_at_any_success_status_answers_500() {
    for status in [200, 201, 202, 204] {
        let failed: Result<i32, _> = Err(ApiError::from(ErrorCode::NotFound).with_status(status));
        let (answered, json) = read(Rt::respond_bare(failed)).await;
        assert_eq!((answered, code(&json)), (500, Some("not_found")), "{status}");
    }
    let failed: Result<i32, _> = Err(ApiError::from(ErrorCode::NotFound).with_status(404));
    assert_eq!(read(Rt::respond_bare(failed)).await.0, 404);
}

/// A refusal at any `2xx` gets the `@http` status, not only one at `200`:
/// a caller reading the status line takes every `2xx` for success. A `1xx`
/// is no final answer and is replaced too; a redirect is kept.
#[tokio::test]
async fn an_http_refusal_at_any_success_status_gets_the_http_status() {
    for status in [200, 202, 100] {
        let refused: Refusal = ApiError::from(ErrorCode::Unauthenticated).with_status(status).into();
        let session = Rt::http_session::<()>(Err(refused)).expect_err("refused");
        assert_eq!(session.status(), StatusCode::UNAUTHORIZED, "{status}");

        let refused: Refusal = ApiError::from(ErrorCode::NotFound).with_status(status).into();
        let access = Rt::http_access(Err(refused)).expect_err("refused");
        assert_eq!(access.status(), StatusCode::FORBIDDEN, "{status}");
    }
    for status in [503, 302] {
        let refused: Refusal = ApiError::from(ErrorCode::Unauthenticated).with_status(status).into();
        let kept = Rt::http_session::<()>(Err(refused)).expect_err("refused");
        assert_eq!(kept.status().as_u16(), status);
    }

    let failed: Result<i32, _> = Err(ApiError::from(ErrorCode::NotFound).with_status(100));
    assert_eq!(read(Rt::respond_bare(failed)).await.0, 500, "a bare failure at 1xx");
}

/// `@http(method: GET, path: "/search", body: JSON)` and the same under
/// `DELETE` and `POST`.
static SEARCH_GET: HttpSpec = spec(HttpMethod::Get, "/search", SLOT_FIELDS, &[], HttpBodyKind::Json);
static SEARCH_DELETE: HttpSpec = HttpSpec {
    method: HttpMethod::Delete,
    ..SEARCH_GET
};
static SEARCH_POST: HttpSpec = HttpSpec {
    method: HttpMethod::Post,
    ..SEARCH_GET
};

async fn search_get(request: Request) -> Handled {
    Ok(echo(Rt::http_input::<Optional>(request, &SEARCH_GET).await?))
}

async fn search_delete(request: Request) -> Handled {
    Ok(echo(Rt::http_input::<Optional>(request, &SEARCH_DELETE).await?))
}

async fn search_post(request: Request) -> Handled {
    Ok(echo(Rt::http_input::<Optional>(request, &SEARCH_POST).await?))
}

/// A JSON-bodied route under a method that need not carry a body reads the
/// query string when there is none — or only whitespace; one that must carry
/// a body still refuses an empty one.
#[tokio::test]
async fn a_json_route_whose_method_needs_no_body_reads_the_query_without_one() {
    let router = Router::new().route("/search", get(search_get).delete(search_delete).post(search_post));
    for method in ["GET", "DELETE"] {
        for body in ["", " \r\n\t"] {
            let (status, json) = send_to(&router, method, "/search?name=q", body).await;
            assert_eq!(
                (status, json),
                (200, json!({ "slot": null, "name": "q" })),
                "{method} {body:?}"
            );
        }
        let (status, json) = send_to(&router, method, "/search?name=q", r#"{"name": "b"}"#).await;
        assert_eq!(
            (status, json),
            (200, json!({ "slot": null, "name": "b" })),
            "{method}: the body wins"
        );
    }
    for body in ["", "  "] {
        let (status, json) = send_to(&router, "POST", "/search?name=q", body).await;
        assert_eq!((status, code(&json)), (400, Some(INVALID)), "POST {body:?}: {json}");
    }
}
