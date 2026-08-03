# Framework integration

`leto` types implement the response traits of actix-web, axum and poem, so a
handler can just return `Result<ApiResponse<T>, ApiError>`.

## actix-web (`actix` feature)

- `ApiResponse<T, M>` implements `Responder`.
- `ApiError<C>` implements `ResponseError` (status `200` by default — see
  [Mapping HTTP status codes](#mapping-http-status-codes) — error in the body).

```rust,ignore
use actix_web::{get, web, App, HttpServer};
use leto::prelude::*;

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
use leto::prelude::*;

async fn get_user(Path(id): Path<u64>) -> Result<ApiResponse<String>, ApiError> {
    let name = find_user(id).ok_or_api_error("user_not_found")?;
    Ok(success(name))
}

let app = Router::new().route("/users/{id}", get(get_user));
```

```rust,ignore
use leto::response_get_api_error;

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

| Framework | Accessor                          |
| --------- | --------------------------------- |
| axum      | `response_get_api_error`          |
| actix-web | `response_get_api_error_actix`    |
| poem      | `response_get_api_error_poem`     |

The value is a `dyn ApiErrorHandler` — the code type is erased, so a service
using a typed code enum is read the same way as one using the default. It
exposes `code()`, `status()`, `has_status()`, `message()`, `meta()`,
`error_source()`, `source()` and the `#[track_caller]` `location()`.

`status()` is the *configured* value; the adapters run it through
`resolve_status` before it reaches the wire, so an out-of-range one is reported
here but replaced there.

## poem (`poem` feature)

- `ApiResponse<T, M>` implements poem's `IntoResponse`.
- `ApiError<C>` implements poem's `error::ResponseError` (status `200` by
  default, error in the body), so `?` inside a `poem::Result` handler produces a
  valid envelope.

```rust,ignore
use poem::{get, handler, web::Path, Route};
use leto::prelude::*;

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
`leto`-flavoured take on `tracing-actix-web`:

- [`Logger`] — wraps each request in a root span, generates a request-id, and
  logs the outcome (calls your error handler on failure).
- [`RequestId`] — a per-request UUID; usable as a `FromRequest` extractor.
- [`RootSpan`] — the request's root `tracing::Span`; usable as an extractor and
  as a re-parenting point.
- `root_span!` — the macro that builds the default OTel-shaped span (customize via
  a `RootSpanBuilder`).

```rust,ignore
use actix_web::App;
use leto::{Logger, error_log};

// default: logs errors at error!(), successes at debug!()
let app = App::new().wrap(Logger::default());

// custom error handler:
let app = App::new().wrap(Logger::with_error_logger(|err| {
    tracing::error!(?err, "request failed");
}));
```

Extract the request-id or root span in a handler:

```rust,ignore
use leto::{RequestId, RootSpan};

async fn handler(id: RequestId, span: RootSpan) -> Result<ApiResponse<()>, ApiError> {
    tracing::info!(%*id, "handling request");
    // ...
    Ok(success(()))
}
```

With `actix-telemetry`, the root span also extracts the incoming OpenTelemetry
context from request headers and records the `trace_id`, so distributed traces
stitch together across services.

## Observability middleware (tower: axum / poem)

The `tower-middleware` feature ships the same request-id + root span for
`tower`-based frameworks, exposed under `leto::tower`:

- `TraceLayer` — a [`tower::Layer`] that, per request, generates a `RequestId`
  (stored in the request extensions), opens the `"HTTP request"` root span with
  the same fields as the actix logger, and records the response status.
- `RequestId` — a per-request UUID; read it in a handler with axum's
  `Extension<RequestId>` (the layer inserts it into the request extensions).

```rust,ignore
use leto::tower::{RequestId, TraceLayer};
use axum::{Router, routing::get, extract::Extension};

async fn handler(Extension(id): Extension<RequestId>) -> String {
    id.to_string()
}

let app = Router::new().route("/", get(handler)).layer(TraceLayer::new());
```

With `tower-telemetry`, the span also extracts the incoming OpenTelemetry
context from request headers and records `trace_id`, mirroring `actix-telemetry`.

poem can consume the same layer through its tower-compat shim:

```rust,ignore
use poem::middleware::TowerLayerCompatExt;
let app = route.with(leto::tower::TraceLayer::new().compat());
```

## Request extractors

The `serde-path` feature exposes framework-specific JSON extractors under
`leto::extract_actix` and `leto::extract_axum`:

- `ApiJson<T>` parses the request body with `deserialize_body`, so malformed JSON
  or a type mismatch becomes a `leto` envelope with an `invalid_body` error.
- `ApiValidated<T>` additionally requires `validator-extract`; it parses,
  validates with `validator::Validate`, and yields `Validated<T>`.

```rust,ignore
use leto::extract_axum::ApiValidated;
use leto::prelude::*;

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
