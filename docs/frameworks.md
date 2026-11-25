# Framework integration

`treat` types implement the response traits of actix-web, axum and poem, so a
handler can just return `Result<ApiResponse<T>, ApiError>`.

## actix-web (`actix` feature)

- `ApiResponse<T, M>` implements `Responder`.
- `ApiError<C>` implements `ResponseError` (status `200` by default — see
  [Mapping HTTP status codes](#mapping-http-status-codes) — error in the body).

```rust,ignore
use actix_web::{get, web, App, HttpServer};
use treat::prelude::*;

#[get("/users/{id}")]
async fn get_user(id: web::Path<u64>) -> Result<ApiResponse<String>, ApiError> {
    let name = find_user(*id).ok_or_api_error("user_not_found")?;
    Ok(success(name))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| App::new().service(get_user))
        .bind(("127.0.0.1", 8080))?
        .run()
        .await
}
```

Because `Result<ApiResponse, ApiError>` is itself a `Responder`, `?` inside the
handler produces a valid envelope automatically.

## axum (`axum` feature)

- `ApiResponse<T, M>` and `ApiError<C>` both implement `IntoResponse`.
- `response_get_api_error(&response)` pulls the type-erased error back out of the
  response extensions (handy in middleware / error logging layers).

```rust,ignore
use axum::{routing::get, extract::Path, Router};
use treat::prelude::*;

async fn get_user(Path(id): Path<u64>) -> Result<ApiResponse<String>, ApiError> {
    let name = find_user(id).ok_or_api_error("user_not_found")?;
    Ok(success(name))
}

let app = Router::new().route("/users/{id}", get(get_user));
```

```rust,ignore
use treat::response_get_api_error;

// in a layer that inspects outgoing responses:
if let Some(err) = response_get_api_error(&response) {
    tracing::warn!(
        code = %err.code(),
        status = err.status(),
        location = %err.location(),
        "request produced an api error",
    );
}
```

Every adapter stashes the error, so the same inspection works on all three; only
the accessor differs, because the response types do:

| Framework | Accessor                       |
| --------- | ------------------------------ |
| axum      | `response_get_api_error`       |
| actix-web | `response_get_api_error_actix` |
| poem      | `response_get_api_error_poem`  |

The value is a `dyn ApiErrorHandler` — the code type is erased, so a service
using a typed code enum is read the same way as one using the default. It
exposes `code()`, `status()`, `has_status()`, `message()`, `meta()`,
`error_source()`, `source()` and the `#[track_caller]` `location()`.

`status()` is the _configured_ value; the adapters run it through
`resolve_status` before it reaches the wire, so an out-of-range one is reported
here but replaced there.

## poem (`poem` feature)

- `ApiResponse<T, M>` implements poem's `IntoResponse`.
- `ApiError<C>` implements poem's `error::ResponseError` (status `200` by
  default, error in the body), so `?` inside a `poem::Result` handler produces a
  valid envelope.

```rust,ignore
use poem::{get, handler, web::Path, Route};
use treat::prelude::*;

#[handler]
async fn get_user(Path(id): Path<u64>) -> poem::Result<ApiResponse<String>> {
    let name = find_user(id).ok_or_api_error("user_not_found")?; // ApiError -> poem::Error
    Ok(success(name))
}

let app = Route::new().at("/users/:id", get(get_user));
```

All three adapters route through the same `ApiError::into_api_response` builder, so
a given error serializes to **byte-identical JSON** regardless of framework
(there's a regression test for exactly this).

## Observability middleware (actix)

The `actix-middleware` feature ships a tracing-aware logger — a trimmed-down,
`treat`-flavoured take on `tracing-actix-web`:

- [`Logger`] — wraps each request in a root span, resolves the request id,
  writes it back on the response as `x-request-id` (see
  [Rejections from inner middleware](#rejections-from-inner-middleware) for
  the one exception), and logs the outcome: the error logger for a failed request, `debug!("request
  success")` otherwise.
- [`RequestId`] — a per-request UUID; usable as a `FromRequest` extractor.
- `X_REQUEST_ID` — the `x-request-id` header name the logger reads and writes.
- [`RootSpan`] — the request's root `tracing::Span`; usable as an extractor and
  as a re-parenting point.
- `RootSpanBuilder` — how the logger opens and closes the root span;
  `DefaultRootSpanBuilder` is the default, `root_span!` the macro that builds
  its span.

In actix the **last** `.wrap()` is the outermost middleware. Register the
`Logger` last so every other middleware runs inside its span.

### Error loggers

The logger calls its error logger for a response that carries an error (a
handler that returned `Err`) and for an `Err` from the inner service.

| Logger               | Level    | Structured fields for                          |
| -------------------- | -------- | ---------------------------------------------- |
| `error_log` (default) | `error!` | `ApiError<&'static str>` (the default code)    |
| `debug_log`          | `debug!` | `ApiError<&'static str>`                       |
| `error_log_for::<C>` | `error!` | `ApiError<C>`, then `ApiError<&'static str>`   |
| `debug_log_for::<C>` | `debug!` | `ApiError<C>`, then `ApiError<&'static str>`   |

The structured fields are `error_code`, `error_location`, `error_hops` (tracked
errors only), `error_source` and the message. actix's `Error` can only be
downcast to a concrete type, so an `ApiError` whose code type the logger does
not name — a typed code enum under the default `error_log`, say — and any
non-`treat` error fall into the generic arm: `request error` with the error's
`Debug` only. A service with a typed code enum uses `error_log_for::<Code>`.

```rust,ignore
use actix_web::App;
use treat::{Logger, error_log_for};

// default: error_log, successes at debug!()
let app = App::new().wrap(Logger::default());

// typed code enum:
let app = App::new().wrap(Logger::with_error_logger(error_log_for::<Code>));

// your own:
let app = App::new().wrap(Logger::with_error_logger(|err: &actix_web::Error| {
    tracing::error!(?err, "request failed");
}));
```

### Custom root span

Implement `RootSpanBuilder` to add fields of your own: build the span with
`root_span!` plus the extra fields, and delegate `on_request_end` to
`DefaultRootSpanBuilder`. Install it with `Logger::<YourBuilder>::new()`, and
give it an error logger with `.error_logger(...)`:

```rust,ignore
use actix_web::dev::{ServiceRequest, ServiceResponse};
use tracing::Span;
use treat::{DefaultRootSpanBuilder, Logger, RootSpanBuilder, error_log_for};

struct TenantSpan;

impl RootSpanBuilder for TenantSpan {
    fn on_request_start(request: &ServiceRequest) -> Span {
        treat::root_span!(request, tenant = tracing::field::Empty)
    }

    fn on_request_end<B>(span: Span, outcome: &Result<ServiceResponse<B>, actix_web::Error>) {
        DefaultRootSpanBuilder::on_request_end(span, outcome);
    }
}

let app = App::new().wrap(Logger::<TenantSpan>::new());
let app = App::new().wrap(Logger::<TenantSpan>::new().error_logger(error_log_for::<Code>));
```

`root_span!(request)` and `root_span!(request, field = value, ...)` work from
any crate, path-qualified (`treat::root_span!`) or imported. Its `request_id` is
the one the `Logger` stored; called outside the logger it generates one for the
span. With `actix-telemetry` it sets the OpenTelemetry parent and `trace_id` —
decided by `treat`'s features, not by the calling crate's.

### Root span fields

| Field                                     | Value                                                             |
| ----------------------------------------- | ----------------------------------------------------------------- |
| `http.method`                             | request method                                                    |
| `http.route`                              | matched route pattern (`/users/{id}`), `default` when none matched |
| `http.flavor`                             | `1.1`, `2.0`, …                                                   |
| `http.scheme`                             | `Forwarded` `proto`, `X-Forwarded-Proto`, URI scheme, `https` on TLS, else `http` |
| `http.host`                               | `Forwarded` `host`, `X-Forwarded-Host`, `Host`, URI authority     |
| `http.client_ip`                          | `Forwarded` `for`, first `X-Forwarded-For` entry, peer address    |
| `http.user_agent`                         | `User-Agent`                                                      |
| `http.target`                             | path **and query string**                                         |
| `http.status_code`                        | the status the client receives                                    |
| `otel.name` / `otel.kind`                 | `HTTP {method} {route}` / `server`                                |
| `otel.status_code`                        | `ERROR` iff the status is a 5xx, else `OK`                        |
| `trace_id`                                | see below                                                         |
| `request_id`                              | the resolved [`RequestId`]                                        |
| `exception.message` / `exception.details` | the error's `Display` / `Debug`, when the response carries one    |

`otel.status_code` follows OpenTelemetry's rule for server spans: a 4xx is the
caller's fault, and an `ApiError` answered at `200` is a handled outcome, so
both are `OK`; only a 5xx is `ERROR`. The tower layer uses the same rule.

See [Privacy and trust](#privacy-and-trust) for `http.target` and the
forwarded headers.

### Rejections from inner middleware

A middleware inside the `Logger` that rejects a request with `Err` — say a
`wrap_fn` returning `ErrorUnauthorized` — has no response yet; actix renders one
further out, past the logger. The logger logs the error, records it on the span,
and passes it on untouched, so outer middleware can downcast it as usual — but
that response carries no `x-request-id`. To keep the id, answer the rejection
as a response instead:

```rust,ignore
// instead of `Err(err)`:
Ok(req.error_response(err))
```

The logger then sees an ordinary response carrying an error: it logs it, and
writes `x-request-id` on it like on any other.

### Extractors

Extract the request id or root span in a handler:

```rust,ignore
use treat::{RequestId, RootSpan};

async fn handler(id: RequestId, span: RootSpan) -> Result<ApiResponse<()>, ApiError> {
    tracing::info!(request_id = %*id, "handling request");
    span.record("tenant", "acme"); // a field your RootSpanBuilder declared
    // ...
    Ok(success(()))
}
```

Both need the `Logger` in front. Without it they reject with a `treat`
envelope at `500` — code `request_id_unavailable` / `root_span_unavailable` —
because that is a wiring bug of the service, not a client error.

### OpenTelemetry

With `actix-telemetry`, the root span extracts the incoming OpenTelemetry
context from the request headers and sets it as the span's parent, so
distributed traces stitch together across services. Two things have to be in
place for that:

- a global propagator, e.g.
  `opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new())`
  — the default one extracts nothing;
- a `tracing-opentelemetry` layer on the subscriber. Without it the span has no
  OpenTelemetry context, and `trace_id` is left empty rather than recorded as
  32 zeros.

The root span this opens is where an error's span trace ends. The trace itself
is not on by default: see [span traces](errors.md#span-traces).

## Observability middleware (tower: axum)

The `tower-middleware` feature ships the same request id + root span for
`tower` stacks that serve `http::Request`s — axum in practice — exposed under
`treat::tower`:

- `TraceLayer` — a [`tower::Layer`] that, per request, resolves a `RequestId`
  (stored in the request extensions), opens the `"HTTP request"` root span with
  the actix logger's fields (differences below), records the response status
  and the `treat` error the response carries, and writes the id back on every
  response as `x-request-id` (unless the handler set it).
- `RequestId` — a per-request UUID; read it in a handler with axum's
  `Extension<RequestId>` (the layer inserts it into the request extensions).
- `X_REQUEST_ID` — the `x-request-id` header name the layer reads and writes.
- `treat::tower::root_span` — the span builder and the field resolvers
  (`request_host`, `request_scheme`, `request_client_ip`, `record_status`,
  `record_api_error`), for a middleware of your own.

It does not log: pair it with
[`treat-axum-logging`](#error-logging-layer-for-axum-treat-axum-logging) for
that.

```rust,ignore
use treat::tower::{RequestId, TraceLayer};
use axum::{Router, routing::get, extract::Extension};

async fn handler(Extension(id): Extension<RequestId>) -> String {
    id.to_string()
}

let app = Router::new().route("/", get(handler)).layer(TraceLayer::new());
```

The span's `otel.status_code` follows the actix rule (`ERROR` iff 5xx), and
`exception.message` / `exception.details` come from the `ApiError` treat's axum
adapter stores in the response extensions (`Display` / `Debug`, as on actix).
Where the fields still differ from actix:

| Field                   | actix                                       | tower                                                                                        |
| ----------------------- | ------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `http.route`, `otel.name` | matched route pattern                     | raw request path: the layer has no router, and axum's `MatchedPath` is not read, so `otel.name` is high-cardinality for paths with ids |
| `http.scheme`           | forwarded headers first, then URI scheme    | URI scheme, then `X-Forwarded-Proto`, else `http`; `Forwarded` is not read                   |
| `http.host`             | forwarded headers first, then `Host`, URI   | URI authority, then `Host`; `Forwarded` / `X-Forwarded-Host` are not read                    |
| `http.client_ip`        | `Forwarded`, `X-Forwarded-For`, peer address | `Forwarded`, `X-Forwarded-For`, then a `std::net::SocketAddr` in the request extensions — axum's `ConnectInfo<SocketAddr>` is a different type and is not read |
| `exception.*`           | any error on the response or `Err`          | only a `treat` error in the response extensions (axum adapter); other errors leave it empty  |

An `Err` from the inner service (never the case for axum's own services, whose
error type is `Infallible`) marks the span `ERROR` and gets no `x-request-id`,
since there is no response to put it on.

With `tower-telemetry`, the span also extracts the incoming OpenTelemetry
context from request headers and records `trace_id`, with the same
requirements as `actix-telemetry` (a global propagator, a
`tracing-opentelemetry` layer; otherwise `trace_id` stays empty).

poem is not covered by this layer: poem's tower-compat shim needs a service over
`poem::Request`, and `TraceService` serves `http::Request`. poem services get
the envelope and error integration (`response_get_api_error_poem`) only.

The root span this opens is where an error's span trace ends. The trace itself
is not on by default: see [span traces](errors.md#span-traces).

## Error logging layer for axum (`treat-axum-logging`)

`treat-axum-logging` is a separate crate (not a `treat` feature) with a
`tower` layer that logs the `treat` error a response carries. It reads the
type-erased error treat's axum adapter stores in the response extensions, so it
logs a typed code enum without knowing it.

- `LoggingLayer::new()` / `default()` — opens a root span (`request`) before
  the inner service runs, and calls `error_log` for a response that carries a
  `treat` error; an `Err` from the inner service is logged as
  `error!("response error")`.
- `LoggingLayer::with_error_logger(f)` — `f: Fn(&dyn ApiErrorHandler) + Clone`
  in place of `error_log`.
- `error_log` — the default: `error!` with the cause chain when the error has a
  source (something failed underneath), `debug!` for a plain business error.
  `info_log` / `debug_log` log every error at one level.
- `root_span_on_request_start` / `root_span_on_response_end` /
  `record_request_id` — the span on its own, for a middleware of your own.
- `event_dynamic_lvl!(level, ...)` — `tracing::event!` with a level held in a
  variable; the caller needs no `tracing` dependency of its own.

Its span is opened at `ERROR` level (so it survives `RUST_LOG=error`) with a
compact field set: `request_method`, `request_uri`, `request_id`,
`http_user_agent`, `http_client_ip`, and on response `http_status_code` and
`http_body_size` (when exact).

The layer never generates a request id; `request_id` is recorded only when
`TraceLayer` has put one in the request extensions, so `TraceLayer` must be the
**outer** layer — in axum the last `.layer` call is the outermost:

```rust,ignore
use treat::tower::TraceLayer;
use treat_axum_logging::LoggingLayer;

let app = Router::new()
    .route("/", get(handler))
    .layer(LoggingLayer::new()) // inner: logs the error, records request_id
    .layer(TraceLayer::new());  // outer: resolves the RequestId first
```

The trade-off: with both layers every request has **two nested spans**
(`HTTP request`, then `request`) and the response gets `x-request-id`; with
`LoggingLayer` alone there is one span and no request id anywhere.

## Privacy and trust

The middleware records what the request says, so mind what ends up in logs:

- `http.target` (actix, tower) and `request_uri` (`treat-axum-logging`) are the
  path **with the query string**. A token or API key passed in the query lands
  in every log line of the request; keep secrets out of URLs, or drop the field
  in your subscriber.
- `http.scheme`, `http.host` and `http.client_ip` (actix, tower) trust the
  `Forwarded` / `X-Forwarded-*` headers, which any client can send. They are
  meaningful only behind a proxy that overwrites them; never authorize on them.
- `http_client_ip` (`treat-axum-logging`) is the raw `X-Forwarded-For` header
  as received — every hop, unparsed, and just as spoofable.

## Request-id propagation

Both middlewares resolve the id the same way, so a request keeps one identifier
across services:

1. If the incoming `x-request-id` parses as a UUID, it is adopted (surrounding
   whitespace is trimmed, since proxies commonly add it) and normalized to the
   canonical lowercase hyphenated form. Every form `uuid::Uuid::parse_str`
   accepts is adopted — braced (`{67e55044-…}`), URN (`urn:uuid:67e55044-…`),
   unhyphenated (`67e5504410b1426f9247bb680e5fe0c8`), and uppercase — but the id
   is re-emitted canonically, not verbatim, so one format holds across services.
2. Otherwise a fresh v4 UUID is generated.
3. The resolved id goes into the request extensions and the span's `request_id`
   field, and is written in canonical form to `x-request-id` on the response —
   unless the handler already set that header, in which case the handler wins.
   The tower layer stamps every response the inner service produces; on actix,
   a rejection an inner middleware returns as `Err` gets no id (see
   [Rejections from inner middleware](#rejections-from-inner-middleware)).

Requiring a UUID is deliberate. `request_id` is recorded on every log line for the
request, so accepting arbitrary header text would let a caller inject unbounded or
misleading content into your logs. A service that must echo a caller-chosen token
of its own format should carry it in a separate header.

Because `RequestId` is always a UUID it stays `Copy` and still derefs to
[`uuid::Uuid`]; build one from a `Uuid` with `RequestId::from`, and go the other
way with `into_uuid()`.

## Request extractors

The `serde-path` feature exposes framework-specific JSON extractors under
`treat::extract_actix` and `treat::extract_axum`:

- `ApiJson<T>` parses the request body with `deserialize_body`, so malformed JSON
  or a type mismatch becomes a `treat` envelope with an `invalid_body` error at
  `422`, with a JSON Pointer when the failure is at a field.
- `ApiValidated<T>` additionally requires `validator-extract`; it parses,
  validates with `validator::Validate`, and yields `Validated<T>`. An invalid
  field is one `errors[]` entry per violated rule, at `422`.

A body that cannot be read at all (over the size limit, a broken stream) is
answered with the envelope too — `invalid_body` with the framework's message —
keeping the framework's status for it, e.g. `413` for a body over the limit.
Every rejection is the envelope, never the framework's plain-text error, and
carries `X-RPC-Status: error` under `rpc-status-header` (see
[RPC status header](#rpc-status-header)).

```rust,ignore
use treat::extract_axum::ApiValidated;
use treat::prelude::*;

async fn create_user(ApiValidated(body): ApiValidated<CreateUser>) -> ApiResponse<User> {
    success(insert_user(body.into_inner()))
}
```

`ApiValidated<T>` exists only for actix-web and axum today. For poem, parse and
validate manually with `deserialize_body` and `Validated::new` if you need the
same contract.

## Mapping HTTP status codes

The default is `200 OK` with the failure in `errors[]`, so nothing changes for
existing callers. When you need real status codes (proxies, status-based
alerting), set one per error:

```rust
error("not_found").with_status(404)
```

All three adapters read it: actix `status_code`, axum's response status, and
poem `status` return the same value, and the serialized body is unchanged (the
status is transport-only).

To keep the mapping with the code type, implement `ApiErrorStatus` on your code
enum and seed the status with `with_code_status`; an explicit `with_status`
called afterwards still wins:

```rust
impl ApiErrorStatus for Code {
    fn status_code(&self) -> u16 {
        match self {
            Code::NotFound => 404,
            Code::RateLimited => 429,
        }
    }
}

error(Code::NotFound).with_code_status() // -> 404
```

## RPC status header

With `rpc-status-header`, every framework adapter adds `X-RPC-Status: ok` or
`X-RPC-Status: error`. Use it when infrastructure needs the operation result
without parsing the JSON body. It follows the envelope's `errors[]` state, not
the HTTP status line.
