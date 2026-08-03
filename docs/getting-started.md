# Getting started

## Install

Depend on the facade crate — it re-exports everything you need. The report type
[`erris`](https://crates.io/crates/erris) comes along as `leto::erris`, so you
don't add it yourself.

```toml
[dependencies]
leto = "0.19.5"

# pick your web framework:
leto = { version = "0.19.5", features = ["actix"] }   # actix-web + middleware
# or
leto = { version = "0.19.5", features = ["axum"] }    # axum
# or
leto = { version = "0.19.5", features = ["poem"] }    # poem
```

The crate builds on **stable Rust**. Only the optional `nightly-provide` feature
requires a nightly toolchain.

### Feature flags

| Feature              | Enables                                                               |
| -------------------- | --------------------------------------------------------------------- |
| `derive` _(default)_ | the `ApiError` / `ApiErrorCode` / `FromErrorMessage` macros           |
| `actix`              | actix-web response traits, extractors, middleware, telemetry, `response_get_api_error_actix` |
| `actix-middleware`   | actix request id, logger, root span                                   |
| `actix-telemetry`    | OpenTelemetry parent-context propagation for actix                    |
| `axum`               | axum `IntoResponse`, extractors, `response_get_api_error`             |
| `poem`               | poem `IntoResponse` + `ResponseError`, `response_get_api_error_poem`  |
| `tower-middleware`   | tower request id + root span middleware for axum/poem                 |
| `tower-telemetry`    | OpenTelemetry parent-context propagation for tower                    |
| `serde-path`         | JSON body decoding with field pointers                                |
| `validator`          | validation helpers that produce `errors[]` with field pointers        |
| `validator-extract`  | `ApiValidated<T>` extractors; also enables `validator` + `serde-path` |
| `meta-slots`         | ready-made typed `meta` payloads (`Pagination`, `RateLimit`)          |
| `openapi`            | `utoipa::ToSchema` bounds on response data                            |
| `spantrace`          | capture a `tracing` span-trace on every error                         |
| `backtrace`          | capture a backtrace on every error                                    |
| `verbose-error`      | always serialize the full cause chain to clients                      |
| `error-status-500`   | default an unset error status to `500` instead of `200`               |
| `rpc-status-header`  | add `X-RPC-Status: ok` / `error` to every framework response          |
| `nightly-provide`    | `std::error::Error::provide` support (**requires nightly**)           |

## Your first handler

A handler returns `ApiResponse<T>` on success or `ApiError` on failure. The `?`
operator does the conversion:

```rust
use leto::prelude::*;

fn get_user(id: u64) -> Result<ApiResponse<String>, ApiError> {
    let name = lookup(id).ok_or_api_error("user_not_found")?;
    Ok(success(name))
}

fn lookup(id: u64) -> Option<String> {
    (id == 1).then(|| "Ada".to_string())
}
```

- `get_user(1)` → `Ok(ApiResponse { data: Some("Ada"), .. })` → `{"data":"Ada"}`.
- `get_user(2)` → `Err(ApiError { code: "user_not_found", .. })` →
  `{"errors":[{"code":"user_not_found"}]}`.

## The prelude

`use leto::prelude::*;` brings in the common surface:

- types — `ApiResponse`, `ApiError`, `ErrorMessage`;
- constructors — `success`, `failure`;
- the `?`-extensions — `OkOrError`, `WrapApiError`, `WithErrorCode`, `ApiErrorTrack`;
- the derive macros (with `derive`).

The free function `leto::error(code)` is **not** in the prelude — call it fully
qualified to avoid clashing with your own `error` names.

## Where to go next

- Model your errors → [Errors](errors.md).
- Shape your responses → [Responses](responses.md).
- Wire it into a server → [Framework integration](frameworks.md).
