# RPC runtime — `treat::rpc`

`treat::rpc` is the runtime that generated RPC code links against instead of
carrying a copy of it: the schema-independent half of a service or client that
a code generator would otherwise paste into every project it emits. It was
extracted from `graphql-rpcgen`'s output, but nothing in it knows about
GraphQL — any generator (or hand-written code) with a registry of error codes,
per-operation code sets and an operation table can use it.

It is one namespace over three crates: the framework-free traits live in
`treat-core` (so the derive macros can reach them through `treat` or
`treat-core`), the `WireEnum` derive in `treat-derive`, and the axum/reqwest
runtime in `treat-rpc`. You only ever write `treat::rpc::...`.

## Features

| Feature | Adds to `treat::rpc` |
|---------|----------------------|
| *(none)* | `WireEnum`, `RuntimeCodes`, `Undeclared`, `Narrow`, `Failure<R>`, `UNDECLARED_CODE_META`, `HttpSpec`, `HttpMethod`, `HttpBodyKind`, `QueryField`, `QueryKind`, `route` |
| `derive` *(default)* | `#[derive(treat::rpc::WireEnum)]` |
| `validator` | `validate::matches_pattern` and the `regex` it takes |
| `rpc` | `Runtime<R>`, the marker the runtime's helpers hang off |
| `rpc-server` | `server`: the axum runtime (implies `rpc`) |
| `rpc-client` | `client`: the reqwest runtime (implies `rpc`), without TLS |
| `rpc-client-rustls` | `rpc-client` with `reqwest`'s `rustls`, for `https` |
| `rpc-stream` | server-sent events and raw byte bodies for whichever of the above is on; re-exports `bytes`, `futures_core`, `futures_util` |
| `rpc-status-header` | with `rpc-server`, `x-rpc-status: ok` on bare (`@http(envelope: false)`), raw and event-stream successes too |

## Codes by wire name

A generated API has one flat **registry** of codes (`ErrorCode`) and, per
operation, the **set** of codes it declares. Both are enums whose values travel
by wire name; `#[derive(WireEnum)]` gives them `ALL`, `from_name` and `as_str`:

```rust
use treat::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, ApiErrorCode, treat::rpc::WireEnum)]
enum GetCodes {
    #[code("things::gone")]
    ThingsGone,
    #[code("internal_error")]
    #[undeclared]
    InternalError,
}
```

A variant's wire name is its `#[code("...")]` — the attribute `ApiErrorCode`
already reads for `Display` — else the name serde writes for it
(`#[serde(rename)]`, or the enum's `#[serde(rename_all)]`), else its
identifier, a raw one unprefixed (`r#type` is `type`, as serde writes it).
When `#[code]` and serde both name a variant they must agree; the build fails
otherwise, and also for a variant serde would write and read under different
names (`serialize`/`deserialize` halves of `rename` or `rename_all` that
differ where the variant does not spell its own name). `#[undeclared]` on one
variant implements `Undeclared`.

`WireEnum` requires `Copy`, and `as_str` takes the value (`code.as_str()` on a
`&Code` still works). `ALL`, `from_name` and `as_str` are trait items, so a
caller needs `treat::rpc::WireEnum` in scope.

`as_str` and `Display` always write one string: `ApiErrorCode`'s `Display`
reads `#[code]`, serde's name and the identifier in the same order, so a
variant serde renames needs no `#[code]`. serde agrees with them wherever it is
told a name (`rename`, `rename_all`) or neither is spelled. A `#[code]` serde
is not told about is still the wire name, while serde keeps writing the
identifier (`#[code("things::gone")] ThingsGone` serializes as
`"ThingsGone"`): add a matching `#[serde(rename)]` if the enum also travels as
a payload. serde's `alias`, `other` and `skip_*` are not mirrored —
`from_name` accepts the wire names only, and `ALL` lists every variant.

## Narrowing a failure

Code below the handlers answers with `Failure<ErrorCode>` — one type, so shared
checks compose with `?`. The handler narrows it to the operation's set:

```rust
use treat::rpc::Narrow;

fn get(id: u64) -> Result<Thing, ApiError<GetCodes>> {
    let thing = lookup(id).narrow()?; // lookup: Result<Thing, Failure<ErrorCode>>
    Ok(thing)
}
```

A declared code becomes the set's variant, message, `meta` and HTTP status
untouched. An undeclared one is a gap in the operation's `@throws`, reported
in every build as the set's `#[undeclared]` value with the raised code in
`meta` under `undeclared_code` — the service's failure, not a dropped
request. Search the logs or the answers for that key to find the gaps. Any registry that is `WireEnum + ApiErrorCode` narrows; the
generated one also implements `RuntimeCodes` to tell the runtime which of its
codes are `invalid_body` and `internal_error`.

## Routes

`treat::rpc::route::by_route(rows, method, path)` finds the operation the router
mounts at a request: over any table of rows implementing `RouteRow`, sorted by
path and then method. It picks the route as the router does — a fixed segment
before a `{param}` or a `{*rest}`; a `{param}` takes one non-empty segment, and
a catch-all `{*rest}`, last in its route, the non-empty rest of the path — and
only then the method, so a method the chosen route lacks is `None` (the
router's `405`) even where another route would fit. `ANY` answers every
method; `HEAD` is not `GET`; a trailing slash is another path (except inside a
catch-all's rest).

The lookup trusts the table's order instead of checking it per request. Check it
once, in a test of the code that owns the table:

```rust
#[test]
fn operation_table_is_well_formed() {
    assert_eq!(treat::rpc::route::check_table(OPERATIONS), Ok(()));
}
```

`check_table` refuses a table that is not sorted by path and then method, one
with a `(path, method)` twice, and an `ANY` row sharing its path with another
row.

## Server (`rpc-server`)

The registry implements `RuntimeCodes`, and the generated server aliases the
runtime once:

```rust
impl treat::rpc::RuntimeCodes for ErrorCode {
    const INVALID_BODY: Self = Self::InvalidBody;
    const INTERNAL_ERROR: Self = Self::InternalError;
}
type Rt = treat::rpc::Runtime<ErrorCode>;
```

Every handler returns `Result<Response, Refusal>`, so each step that can refuse
the request is one `?`. A `Refusal` is the whole response, built from a
`Response` or an `ApiError` with `From`. The shapes, one per kind of operation:

```rust
use treat::rpc::server::{Ctx, QueryField, RawRequest, Refusal, RAW_BODY_LIMIT};
use treat::rpc::HttpSpec;

// RPC: `POST` + JSON body, answered with the envelope.
async fn rename<S: UsersService>(
    State(service): State<S>,
    Ctx(ctx): Ctx<S::Ctx>,
    request: Request,
) -> Result<Response, Refusal> {
    ctx.require_role(Role::Admin)?;
    let input: RenameInput = Rt::json_input(request).await?; // `Rt::no_input(request).await?` without one
    Ok(Rt::respond(service.rename(&ctx, input).await))
}

// `@http`: its own method and path; callers read the status line. One
// `static` per operation describes it, and the client takes the same one.
// `HttpSpec::get` (`::new(method, path)`, `::post`, ...) starts from the
// directive's defaults — the body kind its method implies, the envelope on —
// and each builder changes one field: `.body(HttpBodyKind::Form)`, `.bare()`.
static FIND: HttpSpec = HttpSpec::get("/api/users/{id}")
    .fields(&[QueryField::int("id"), QueryField::str("tags").list(), QueryField::json("filter")])
    .path_fields(&["id"])
    .bare();

async fn find<S: UsersService>(
    State(service): State<S>,
    ctx: Result<Ctx<S::Ctx>, Refusal>,
    request: Request,
) -> Result<Response, Refusal> {
    let ctx = Rt::http_session(ctx)?; // 401 unless the refusal has a status
    Rt::http_access(ctx.require_role(Role::Admin))?; // 403 likewise
    let input: FindInput = Rt::http_input(request, &FIND).await?; // 400 likewise
    Ok(Rt::respond_http(&FIND, service.find(input).await)) // bare or enveloped, as `FIND` says
}

// `@subscription`: `GET` + query string, Server-Sent Events back.
async fn watch<S: UsersService>(State(service): State<S>, RawQuery(query): RawQuery) -> Result<Response, Refusal> {
    let input: WatchInput = Rt::query_input(query.as_deref(), WATCH_FIELDS)?;
    Ok(Rt::respond_events(service.watch(input).await))
}

// `@raw(response:)`: a JSON body in, a `RawBody` out.
async fn export<S: UsersService>(State(service): State<S>, request: Request) -> Result<Response, Refusal> {
    let input: ExportInput = Rt::json_input(request).await?;
    Ok(Rt::respond_raw(service.export(input).await))
}

// What `@raw(request: [...])` declares.
const UPLOAD_TYPES: &[&str] = &["text/csv", "application/json"];

// `@raw(request:)`, buffered: the query string, then the body — its type
// checked (415) and its size bounded (413) — then the rules.
async fn upload<S: UsersService>(State(service): State<S>, request: Request) -> Result<Response, Refusal> {
    let mut input: UploadInput = Rt::input_from_query(request.uri().query(), UPLOAD_FIELDS)?;
    input.file = Rt::raw_body(request, UPLOAD_TYPES, RAW_BODY_LIMIT).await?;
    Rt::validate(&input)?;
    Ok(Rt::respond(service.upload(input).await))
}

// `@raw(request:, stream: true)`: the type checked first, the body unread;
// the service bounds it.
async fn ingest<S: UsersService>(State(service): State<S>, raw: RawRequest) -> Result<Response, Refusal> {
    Rt::require_content_type(raw.headers(), INGEST_TYPES)?;
    let input: IngestInput = Rt::query_input(raw.query(), INGEST_FIELDS)?;
    Ok(Rt::respond(service.ingest(input, raw).await))
}
```

An `@http` operation's `HttpSpec` holds its route, its field table (how each
field is spelled in a query string, a form body or a path), which path
segments are input fields, where the rest travel and whether a success is the
envelope. A field the table names is read from a query string or a form as
the table types it, and one it leaves out is not read at all; a `list` field
nobody sent — an empty list included, which the client does not send — is left
out of the input, so it needs `#[serde(default)]` like any optional field.
`http_input` reads the path parameters first and lets them win over
any other field of the same name; each is typed as its table entry says, and
one the table leaves out is read as a string, whatever the body kind. A form
body is read only when the `Content-Type`'s media type is exactly
`application/x-www-form-urlencoded` (any case, any parameters); the query
string is read either way, and the body wins per field. `respond_http`
answers an enveloped operation as `respond` does — the service returns an
`ApiResponse` — and a bare one as `respond_bare`: the value itself, and a
failure at any `2xx` (or `1xx`) status at `500`, since only the status tells
it from a success. A JSON-bodied `GET`, `DELETE` or `ANY` route called without a body
(or with only whitespace) reads its input from the query string instead; a route without path fields does not
read the path at all.

The context is the project's: it implements `ResolveContext` (resolve the
caller from the request parts, or refuse), and the generated server's
`FromRequestContext: ResolveContext` adds the schema's `require_role` /
`require_permission`.

What the runtime answers when it refuses a request, always with the registry's
`INVALID_BODY` code, spelled by its wire name (`as_str`) whichever step
refused, even where the registry's `Display` differs:

| Refusal | RPC route | `@raw(request:)` route | `@http` route |
|---------|-----------|------------------------|---------------|
| body without a JSON `Content-Type` (with `source.header`) | `415` | — | not checked |
| body under a `Content-Type` the operation does not declare (with `source.header`) | — | `415` | — |
| body over the router's `DefaultBodyLimit` | `413` | — | `413` |
| buffered body over `raw_body`'s limit (`RAW_BODY_LIMIT`) | — | `413` | — |
| body that broke off while being read (the connection dropped) | `400` | `200` (buffered) | `400` |
| JSON body that does not parse (with a `source.pointer`) | `422` | — | `400` |
| query string, form or path that does not read (with a `source.parameter`, see below) | `200` | `200` | `400` |
| an input breaking a `validator` rule (with a `source.pointer`) | `200` | `200` | `400` |

A value read from a query string, a form or a path is named in
`source.parameter` whether its text is not of its field's kind (`limit=three`
for an `Int`) or serde refuses it as the field's type (`id=not-a-uuid` for a
`Uuid`, `limit=300` for a `u8`): the parameter is the top-level key the failure
lies under, also when it lies inside a `Json` field's value or a list's item.
A query or form value that is not UTF-8 once decoded (`name=caf%E9`) names its
key too. Some refusals name none: a missing required field, a query or form
key that is not UTF-8 itself, and any path refusal from axum — a path
parameter that is not UTF-8 once decoded, or an operation with path fields run
outside a router's matched route (a `route_service`, a test's `oneshot`).
axum refuses such a path as a whole and does not hand the runtime a
parameter's name; what it says is only in the message the refusal carries
(`Invalid UTF-8 in ...`).

An RPC route leaves the rest at the envelope's status, since its callers read
`errors[]`; an `@http` route answers a context refusal `401`, an access check's
`403` and a bare failure `500`, each unless the refusal carries a status of its
own. Under `treat`'s `error-status-500` an `ApiError` without one already
carries `500`, which is then kept: set `401`/`403` on such a refusal yourself.

**`Content-Type` and cross-origin requests.** A browser sends a cross-origin
request without asking the server first (a CORS preflight) only when it is a
"simple" one — a `POST` among them — whose content type is a form's or plain
text's: `application/x-www-form-urlencoded`, `multipart/form-data`,
`text/plain`. Such a request carries the visitor's cookies, and runs before
CORS can hide the answer, so a route that reads a body under one of those types
can be driven by any page the visitor opens. What each kind of route does:

- **RPC** — `json_input` and `no_input` read a body only under
  `application/json` or a `+json` type (any case, any parameters) and refuse
  anything else at `415` before reading it, so a cross-origin call needs a
  preflight. Clients must send the header; `RpcClient` does.
- **`@raw(request: [...])`** — `raw_body` reads a buffered body only under a
  type the operation declares, and a streamed one's handler checks the same
  with `require_content_type` before the service gets the body; both refuse at
  `415`. Each declared entry is a media range: `text/csv` takes that type,
  `image/*` any subtype of `image`, `*/*` any type — but never a request that
  declares none. The check is only as strong as the list: an operation
  declaring `text/plain`, a form type, or a range covering one (`text/*`,
  `*/*`) is reachable from a cross-origin form and needs a guard of its own.
- **`@raw(response:)`** reads its input as JSON, like an RPC.
- **`@subscription`** is a `GET`: a cross-origin page can open one but not read
  it, so the service must not change state on one.
- **`@http`** checks nothing, by design: it keeps the contract its existing
  callers have, often a plain form post. One a cross-origin page must not reach
  needs its own guard — a `SameSite` session cookie, an anti-forgery token, an
  `Origin` check.

With `rpc-stream`, `RawBody` (bytes, a content type, headers, `attachment(..)`)
is what a `@raw(response:)` service returns; `Rt::raw_response` /
`Rt::respond_raw` turn it into a response, refusing a content type that is not
a header value with `INTERNAL_ERROR`. Its headers are appended in order, so a
name listed twice (`Set-Cookie`) is sent twice; `content-type`,
`content-length` and `x-rpc-status` among them are not sent
(`RAW_BODY_IGNORED_HEADERS`), since the runtime writes those from the body and
the outcome. With `rpc-status-header`, bare, raw and event-stream successes
carry `x-rpc-status: ok` like every successful envelope.

`Rt::respond_events` / `sse_response` write each event as one JSON `data:`
frame. An event that does not serialize as JSON ends the stream there: the
answer is already committed to `200`, so the body fails and the connection
drops, which the client reports as `StreamError::Transport` after the events
before it (or `ClientError::Transport` if no byte of the answer had gone out
yet). The runtime has no logger; a service whose events can fail to serialize
turns them into JSON itself, and logs and skips the ones that do not.

## Client (`rpc-client`)

A generated client is `pub struct ApiClient(RpcClient)` with an accessor per
service, and each method is one call:

| Operation | Method |
|-----------|--------|
| RPC | `call(path, &input)` |
| `@http` | `call_http(&SPEC, &input)`, `SPEC` the server's `HttpSpec` |
| `@subscription` | `subscribe(path, FIELDS, &input)` → `EventStream<T>` |
| `@raw(response:)` | `raw(path, &input)` → `reqwest::Response` |
| `@raw(request:)` | `raw_request(path, FIELDS, &input, body, content_type)` → `reqwest::Response` |
| `@raw(request:)` with an envelope back | `raw_request_json(path, FIELDS, &input, body, content_type)` |

`FIELDS` is the same `&[QueryField]` table the server reads with (for
`@http`, the spec's `fields`): a field of kind `Json` travels as JSON text (the
string `abc` as `"abc"`), only a `list` field repeats its key, and a non-list
array is one JSON-text value. A list of another kind cannot spell a `null`
item, so one is `Encode` rather than the text `null`. A `null` value and an
empty list are not sent at all — a repeated key has no spelling for zero
items — and the server leaves an absent parameter out of the input it
rebuilds, so every `list` field of an input that travels in the URL (a query
string, a form body) needs `#[serde(default)]` on the server's input type, or
an empty list is refused as a missing field (an `Option` list reads back as
`None`). An operation without an input passes `NoInput`, which serializes as `{}`. Every JSON body goes with
`Content-Type: application/json`; a raw body with the `content_type` the caller
passes, which must be one the operation declares.

`call_http` sends the spec's method — for `ANY`, `GET` when the input travels
without a body and `POST` when it travels in one (`HttpSpec::request_method`)
— to its path, with each `path_fields` entry filled from the input and removed
from it, the rest travelling as `body` says. A `list` field in the path is its
one item, spelled as that item's kind, which is how the server reads it back;
an array of any other length is `Encode`. Under `GET` or `DELETE` a `Form`
input goes in the query string, which the server reads under the same table,
rather than in a body those methods do not define; a `Json` input stays the
JSON body, which the server reads under any method (its query-string fallback
reads only the fields the table names, so the client does not rely on it).

An envelope's `data` is handed to the declared type whatever it holds,
`null` included: a server answers an operation that returns `()`, `None` or a
null value with `{"data": null}`, which reads back as `()`, `None` or
`Value::Null`, and is `Decode` only for a type that cannot be `null`. An
envelope with neither `data` nor `errors[]` is no answer.

A failure is `ClientError<C>`, `C` the operation's code set:

- `Api(ApiErrors<C>)` — the server's `errors[]`, whatever the status line says;
  `errors.code::<ErrorCode>()` reads the first code against the registry (or
  `C`), `errors.is(code)` and `meta_for(code)` against the set;
- `Encode` — nothing was sent: the input did not serialize, a path parameter
  is missing, `null` or no URL segment (`""`, `.`, `..`), a list path
  parameter holds other than one item, a list that is not `Json` holds `null`,
  or `reqwest` could not build the request (a URL or header value that is not
  one);
- `Transport` — no answer: connection, TLS, timeout, a body cut off;
- `Decode` — a `2xx` answer that is not the envelope, the bare value, or the
  stream asked for;
- `Status(u16)` — a non-`2xx` answer without a usable envelope, such as a
  proxy's page.

`ClientError` and `StreamError` are `#[non_exhaustive]`.

How a refusal is told from a success where the success is not the envelope:

- `x-rpc-status: error` (`treat`'s `rpc-status-header` on the server) makes
  any answer a refusal, whatever its status: an envelope carrying `data`
  instead of `errors[]` is then no answer (`Decode`, or `Status` off `2xx`),
  not a success. `ok` decides less: it lets a `2xx` raw answer through as its
  bytes without the peek below, and changes nothing else — a raw answer under
  another status is still a refusal, a subscription still opens only on an
  event stream, and an envelope's `errors[]` is still `Api`;
- a subscription opens only on a `2xx` `text/event-stream` answer; anything
  else is read as a refusal, so a proxy's page is `Decode` rather than an empty
  stream;
- a raw answer is its bytes unless its media type is exactly
  `application/json` (`application/jsonl` is not); such an answer is read up
  to `RpcClient::RAW_PEEK_LIMIT` (64 KiB), and is a refusal if it ends within
  that and is an envelope with `errors[]`. Otherwise it is handed back rebuilt
  — status, headers, extensions (`remote_addr()` among them) and `url()` as
  they arrived — with a body of the bytes read followed by the rest, still
  streaming. Turn on `rpc-status-header` to have JSON raw answers skip even
  that peek.
- an answer read as a refusal (any non-`2xx` one, `x-rpc-status: error`, a
  subscription answered without an event stream) is read up to
  `RpcClient::REFUSAL_LIMIT` (1 MiB) by every method, `call` and `call_http`
  included; one going past it is `Decode` under a `2xx` status and `Status`
  under any other, the rest left unread. A success is not bounded: a `2xx`
  envelope or bare value not declared `error` is the operation's answer, and
  `call` and `call_http` read it whole.

The SSE reader follows the specification's line rules (`\r`, `\n`, `\r\n`,
split anywhere between chunks; a `data` line without a colon; a leading byte
order mark), looks at each byte once, and holds at most
`RpcClient::SSE_BUFFER_LIMIT` (16 MiB) waiting for an event to end: past it the
stream yields `StreamError::Decode` and ends. A frame that is not the declared
type is `StreamError::Decode` and the stream goes on. Nothing reconnects.

### TLS

`treat` builds `reqwest` without its default features, TLS, HTTP/2 and system
proxies among them. Without a TLS feature an `https` base URL fails when a
request is sent, as `ClientError::Transport`. Turn on `rpc-client-rustls`
(`reqwest`'s `rustls`: aws-lc-rs and the platform verifier), or depend on
`reqwest` yourself with the features you want — Cargo unifies them — and pass a
configured client to `RpcClient::with_http`.

## Migrating from code `graphql-rpcgen` generated with its own runtime

- **Context.** The project's `impl FromRequestContext for MyContext { fn
  from_parts(..) -> Result<Self, Response> }` becomes `impl ResolveContext for
  MyContext { fn from_parts(..) -> Result<Self, Refusal> }`; the generated
  `FromRequestContext: ResolveContext` keeps only the access checks, which
  return `Result<(), Refusal>` too.
- **`Refusal` instead of `Response`.** A check's refusal is
  `Err(ApiError::from(ErrorCode::Forbidden).into())`, or a ready `Response`
  `.into()`.
- **`RawBody`** is no longer `IntoResponse`: a service returns it, and
  `Rt::raw_response` / `Rt::respond_raw` answer with it.
- **JSON bodies.** An RPC body without a JSON `Content-Type` is refused at
  `415`. A body that does not parse carries the registry's `INVALID_BODY`
  rather than `treat`'s own `invalid_body` (still `422` on an RPC route; `400`,
  was `422`, on an `@http` one), and one over the body limit is the envelope at
  `413` instead of axum's plain-text answer.
- **Query strings.** A `Json` field's string is sent quoted and a non-list
  array as one JSON value; a client of your own must spell them so.
- **`@http` operations** are described once, by a `static HttpSpec` (method,
  path, field table, path fields, body kind, envelope), which the server and
  the client share: `Rt::http_input(request, &SPEC)` and
  `Rt::read_http_input(request, &SPEC)` replace the forms taking the table,
  path fields and body kind; `Rt::respond_http(&SPEC, result)` picks between
  `respond` and `respond_bare`; `client.call_http(&SPEC, &input)` replaces
  the seven-argument form and decides the method an `ANY` route is sent with.
  A path parameter the table leaves out is read as a string under every body
  kind (a form or query-string route used to drop it). A form body is read
  only under exactly `application/x-www-form-urlencoded`, no longer under any
  type that starts with it.
- **Raw request bodies.** `Rt::raw_body(request, TYPES, limit)` takes the
  request and the operation's declared types instead of the body, so the
  handler takes `request: Request` and reads the query string from
  `request.uri().query()` first. A body under a type the operation does not
  declare is refused at `415`, a streamed one's handler included
  (`Rt::require_content_type(raw.headers(), TYPES)?`); a buffered body over
  the limit answers `413` (was `200`).
- **`RawBody` headers** are appended rather than inserted, so a repeated name
  is sent each time; `content-type`, `content-length` and `x-rpc-status` in
  them are no longer sent.
- **Bare successes** carry `x-rpc-status: ok` under `rpc-status-header`.
- **Client errors.** `ClientError` is `#[non_exhaustive]` and gains `Encode`
  for failures before sending (a missing or `null` path parameter included);
  a `2xx` envelope without `errors[]` where bytes or a stream were expected is
  `Decode` (was `Status(200)`); `errors.code()` is `errors.code::<ErrorCode>()`.
  `subscribe`, `raw_request` and `raw_request_json` take the operation's field
  table, `call_http` its `HttpSpec`. `errors.is(code)` and `meta_for(code)`
  compare wire names (`WireEnum::as_str`) and need the set to derive
  `WireEnum`, which a generated set does. A JSON raw answer that is not a
  refusal keeps its `url()` and `remote_addr()`.
- **TLS** is the application's choice: `rpc-client-rustls`, or a `reqwest`
  feature of your own.
