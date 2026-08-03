# Leto

`leto` is a Rust crate for RPC-style responses over HTTP.

The closer references are gRPC, where transport delivery and the RPC result are
separate, and GraphQL, where execution failures are returned in the response
body through `errors[]`.

It is not JSON:API. JSON:API is a large specification: a document format for
resources with typed identity, relationships, compound documents, sparse
fieldsets, sorting, pagination and filtering rules, and its own media type with
content negotiation. Adopting it means adopting all of that. leto is a plain
response format — one small envelope, an operation result inside it.

## Purpose

Most service APIs have more outcomes than the HTTP status registry can express:

- `user_not_found`
- `coupon_expired`
- `wrong_tenant`
- `payment_declined`
- `invalid_email`
- `quota_exceeded`

`leto` follows the RPC-over-HTTP boundary: HTTP describes delivery, while the
JSON envelope describes the operation result.

## Contract

Every response uses the same top-level shape.

Success responses contain `data` and may contain `meta`:

```json
{ "data": { ... }, "meta": { ... } }
```

Failure responses contain a non-empty `errors[]` array:

```json
{ "errors": [ { "code": "...", "message": "...", "source": { "pointer": "/email" }, "meta": { ... } } ] }
```

The serialization rules are deliberately small:

- success has `data`;
- failure has non-empty `errors[]`;
- `meta` is optional;
- empty fields are omitted;
- `data` and `errors[]` do not coexist in the serialized response.

Clients parse one schema and branch on `errors[]` and `code`. They do not infer
domain meaning from the HTTP status line.

## Error Model

`errors[]` is the primary failure channel. It is an array because one request can
fail for more than one reason, especially during validation or bulk operations.

Each error entry can contain:

| Field              | Purpose                                                   |
| ------------------ | --------------------------------------------------------- |
| `code`             | Stable machine-readable value. Clients match on this.     |
| `message`          | Human-readable text. Clients do not use it as logic.      |
| `source.pointer`   | JSON Pointer to the invalid request field.                |
| `source.parameter` | Query parameter that caused the failure.                  |
| `source.header`    | Header that caused the failure.                           |
| `type`             | Stable documentation URI, compatible with RFC 9457 style. |
| `instance`         | Identifier for this occurrence, often a request id.       |
| `meta`             | Structured service-specific detail.                       |

Server-side `ApiError` also keeps the original `erris` source chain and the
`#[track_caller]` location. Those details are for logs and diagnostics. By
default the client receives only the top-level API error; verbose serialization
is explicit.

## HTTP Status Policy and RPC Over HTTP

The default error status is `200 OK`.

That default follows the boundary used by RPC protocols over HTTP: HTTP
describes the transport exchange, while the response envelope describes the
operation result. If the service received the request, evaluated it, and returned
a complete JSON envelope, the exchange succeeded. A domain refusal such as
`coupon_expired`, `wrong_tenant`, or `quota_exceeded` is represented in
`errors[]`.

This is different from REST, where methods, status codes, cache behavior,
resource URIs, and intermediaries are part of the application contract. REST is
the right model for resource-oriented APIs and distributed hypermedia. `leto`
targets JSON-over-HTTP operations where service-specific failures often do not
fit the HTTP status registry.

The same split appears in other protocols:

- gRPC normally carries a successful HTTP/2 exchange as `:status 200` and puts
  the RPC result in `grpc-status`.
- GraphQL represents execution errors in the response body through `errors[]`
  once the operation was executed.

The reason is practical. Infrastructure acts on HTTP status before application
code reads the body: gateways can replace `5xx` bodies, retry policies and
circuit breakers can react to `5xx`, caches and proxies can apply protocol
rules, and generic middleware can count `4xx` / `5xx` against error budgets.
That is correct for transport and infrastructure failures, but it is the wrong
signal for a typed application result.

Use real HTTP statuses when the condition belongs to the transport layer or must
be visible to intermediaries:

| Condition                                                                   | Status        |
| --------------------------------------------------------------------------- | ------------- |
| Authentication or authorization must be handled by browser/gateway behavior | `401` / `403` |
| Resource routing must report absence                                        | `404`         |
| Rate limiting must be visible to generic clients or gateways                | `429`         |
| Request body failed before business logic ran                               | `400` / `422` |
| Server failed and the response cannot be trusted                            | `5xx`         |

`ApiError` supports this as a transport hint:

```rust
let err = leto::error("invalid_email").with_status(422);
```

The framework adapter applies that status. It is not serialized into the body
and it does not replace `errors[]`. The tradeoff is explicit: clients must read
the envelope to determine the operation result — or enable `rpc-status-header`,
which puts that answer in a header the way gRPC puts it in a trailer.

## Why This Shape

The envelope exists because application errors need a richer contract than the
HTTP status line can provide:

- clients match stable machine codes;
- validation can return one error per invalid field;
- bulk operations can return every relevant failure;
- logs keep source chains, caller locations, and framework-consistent response
  data.

That gives ordinary JSON-over-HTTP services an explicit operation result model:
`data` for success, `errors[]` for application failure, and HTTP status for
delivery semantics.

## Quick Example

```rust
use leto::prelude::*;

fn find_user(id: u64) -> Result<ApiResponse<User>, ApiError> {
    let user = db_lookup(id).ok_or_api_error("user_not_found")?;
    Ok(success(user))
}
```

Success:

```json
{ "data": { "...": "..." } }
```

Failure:

```json
{ "errors": [{ "code": "user_not_found" }] }
```

The handler uses ordinary `?` flow. `Option`, `bool`, `Result<T, E>`, and
`erris::Report` can be converted into `ApiError` through extension traits.

### Typed error codes

Use `ApiErrorCode` for the wire contract and keep handler results typed as
`leto::ApiError<ApiErrorCode>`.

```rust
use serde::{Deserialize, Serialize};

pub type ApiError = leto::ApiError<ApiErrorCode>;

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, leto::ApiErrorCode)]
pub enum ApiErrorCode {
    database_error,
    illegal_session,
    invalid_argument,
    maintenance,
    shutdown,
    unauthorized,
}

pub async fn info() -> Result<leto::ApiResponse<InfoResponse>, ApiError> {
    let info = load_info().await.wrap_api_error(ApiErrorCode::database_error)?;
    Ok(leto::success(info))
}
```

## What The Crate Provides

- `ApiResponse<T, M>`: typed `{ data, meta, errors[] }` envelope.
- `ApiError<C>`: typed error with code, message, metadata, source chain,
  transport status hint, and caller location.
- `ErrorMessage`: serialized client-facing error entry.
- Derives for typed error enums: `ApiError`, `ApiErrorCode`,
  `FromErrorMessage`.
- Framework adapters for actix-web, axum, and poem.
- Request extractors that can report JSON parse and validation failures with
  field locators.
- OpenAPI schemas through `utoipa`.
- Request id, tracing root span, and OpenTelemetry middleware for actix and
  tower-based stacks.
- Ready-made typed `meta` slots such as pagination and rate limits.

## Workspace Layout

| Crate                               | Purpose                                                                   |
| ----------------------------------- | ------------------------------------------------------------------------- |
| [`leto`](crates/leto)               | Facade crate. Depend on this in applications.                             |
| [`leto-core`](crates/leto-core)     | Envelope, errors, conversions, adapters, extractors, validation support.  |
| [`leto-derive`](crates/leto-derive) | Proc macros for typed errors and response parsing.                        |
| [`leto-actix`](crates/leto-actix)   | actix-web middleware: logger, request id, root span, OpenTelemetry.       |
| [`leto-tower`](crates/leto-tower)   | tower middleware for axum and poem: request id, root span, OpenTelemetry. |

Two more members are not published. They are compile-only: the derive macros must
emit absolute paths, so a bare `ApiResponse` or a hardcoded `leto::` breaks the
build here instead of downstream.

| Crate                                                 | Checks that generated code resolves                   |
| ----------------------------------------------------- | ----------------------------------------------------- |
| [`leto-hygiene`](crates/leto-hygiene)                 | without the prelude, and without the facade in scope. |
| [`leto-hygiene-renamed`](crates/leto-hygiene-renamed) | when the facade is renamed to `envelope`.             |

## Features

The `leto` facade re-exports functionality behind feature flags.

| Feature             | Enables                                                             |
| ------------------- | ------------------------------------------------------------------- |
| `derive`            | Derive macros. Enabled by default.                                  |
| `actix`             | actix-web response traits, extractors, middleware, telemetry.       |
| `axum`              | axum `IntoResponse`, extractors, response inspection.               |
| `poem`              | poem response integration.                                          |
| `openapi`           | `utoipa::ToSchema` support.                                         |
| `serde-path`        | JSON body errors with field pointers.                               |
| `validator`         | Validation helpers that produce `errors[]`.                         |
| `validator-extract` | Framework extractors that parse and validate request bodies.        |
| `meta-slots`        | Typed `meta` helpers such as pagination and rate limits.            |
| `actix-middleware`  | actix request id, logger, and root span.                            |
| `actix-telemetry`   | OpenTelemetry context propagation for actix.                        |
| `tower-middleware`  | tower request id and root span middleware.                          |
| `tower-telemetry`   | OpenTelemetry context propagation for tower.                        |
| `spantrace`         | Capture tracing span traces on errors.                              |
| `backtrace`         | Capture backtraces on errors.                                       |
| `verbose-error`     | Serialize full cause chains to clients.                             |
| `error-status-500`  | Default an error with no explicit status to `500` instead of `200`. |
| `rpc-status-header` | Add an `X-RPC-Status: ok` / `error` header to every response.       |
| `nightly-provide`   | `std::error::Error::provide` support. Requires nightly.             |

The crate builds on stable Rust unless `nightly-provide` is enabled.

## Documentation

- Handbook: [`docs/`](docs/), starting at [`docs/README.md`](docs/README.md).
- API reference: [docs.rs/leto](https://docs.rs/leto).

## Development

```sh
just test
just lint
```

```sh
RUSTFLAGS= cargo +stable test
```

## License

MIT. See [`LICENSE`](LICENSE).

`leto-actix` is an adapted derivative of
[`tracing-actix-web`](https://github.com/LukeMathWalker/tracing-actix-web) by
Luca Palmieri, used under the MIT License. See [`NOTICE`](NOTICE).
