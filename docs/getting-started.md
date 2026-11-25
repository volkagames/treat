# Getting started

## Install

Depend on the facade crate — it re-exports everything you need. The report type
[`erris`](https://crates.io/crates/erris) comes along as `treat::erris`, so you
don't add it yourself.

```sh
cargo add treat

# pick your web framework:
cargo add treat --features actix   # actix-web + middleware
# or
cargo add treat --features axum    # axum
# or
cargo add treat --features poem    # poem
```

The crate builds on **stable Rust**. Only the optional `nightly-provide` and
`tracked` features require a nightly toolchain.

### Feature flags

| Feature              | Enables                                                                                                                  |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `derive` _(default)_ | the `ApiError` / `ApiErrorCode` / `FromErrorMessage` / `rpc::WireEnum` macros                                            |
| `actix`              | actix-web response traits, middleware, telemetry, `response_get_api_error_actix`                                         |
| `actix-middleware`   | actix request id, logger, root span                                                                                      |
| `actix-telemetry`    | OpenTelemetry parent-context propagation for actix                                                                       |
| `axum`               | axum `IntoResponse`, `response_get_api_error`                                                                            |
| `poem`               | poem `IntoResponse` + `ResponseError`, `response_get_api_error_poem`                                                     |
| `tower-middleware`   | tower request id + root span middleware for axum, as `treat::tower` (poem is not covered)                                |
| `tower-telemetry`    | OpenTelemetry parent-context propagation for tower                                                                       |
| `serde-path`         | JSON body decoding with field pointers; with `actix` / `axum`, the `ApiJson<T>` extractors (`extract_actix` / `extract_axum`) |
| `validator`          | validation helpers that produce `errors[]` with field pointers                                                           |
| `validator-extract`  | with `actix` / `axum`, the `ApiJson<T>` and `ApiValidated<T>` extractors; also enables `validator` + `serde-path`         |
| `meta-slots`         | ready-made typed `meta` payloads (`Pagination`, `RateLimit`)                                                             |
| `openapi`            | `utoipa::ToSchema` bounds on response data                                                                               |
| `spantrace`          | span-trace on every error; also needs `ErrorLayer`, see [errors.md](errors.md#span-traces)                               |
| `backtrace`          | capture a backtrace on every error                                                                                       |
| `verbose-error`      | always serialize the full cause chain to clients                                                                         |
| `error-status-500`   | default an unset error status to `500` instead of `200`                                                                  |
| `rpc-status-header`  | add `X-RPC-Status: ok` / `error` to every envelope and `ApiError` response; with `rpc-server`, to bare, raw and SSE successes too |
| `rpc`                | `treat::rpc::Runtime` for generated RPC code (see [rpc.md](rpc.md))                                                      |
| `rpc-server`         | the axum server runtime for generated RPC code; implies `rpc`                                                            |
| `rpc-client`         | the reqwest client runtime for generated RPC code; implies `rpc`                                                         |
| `rpc-client-rustls`  | `rpc-client` with `reqwest`'s `rustls`, for `https` base URLs                                                            |
| `rpc-stream`         | server-sent events and raw byte bodies for the RPC server / client; implies `rpc`                                       |
| `tracked`            | `erris::Result<T, ApiError>` as a handler result, each `?` recorded as a hop (**requires nightly**; see [errors.md](errors.md#tracking-better-debug-trails)) |
| `nightly-provide`    | `std::error::Error::provide` support (**requires nightly**)                                                              |

The request extractors need two features: a framework (`actix` / `axum`) and
`serde-path` (for `ApiJson`) or `validator-extract` (for both `ApiJson` and
`ApiValidated`). A framework feature alone gives the response adapters only.

## Your first handler

A handler returns `ApiResponse<T>` on success or `ApiError` on failure. The `?`
operator does the conversion:

```rust
use treat::prelude::*;

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

`use treat::prelude::*;` brings in the common surface:

- types — `ApiResponse`, `ErrorResponse`, `ApiError`, `ErrorMessage`, the
  `ApiErrorCode` trait, `NoData` / `NoMeta`;
- constructors — `success`, `success_with_meta`, `failure`;
- the `?`-extensions — `OkOrError`, `WrapApiError`, `WithErrorCode`,
  `ApiErrorTrack`, `ApiResponseTrack`;
- the derive macros (with `derive`).

The free function `treat::error(code)` is **not** in the prelude — call it fully
qualified to avoid clashing with your own `error` names.

## Where to go next

- Model your errors → [Errors](errors.md).
- Shape your responses → [Responses](responses.md).
- Wire it into a server → [Framework integration](frameworks.md).
