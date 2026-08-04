# treat — handbook

Practical documentation for the `treat` crate: an HTTP response **envelope**
(`{ data, meta, errors[] }`) with a **typed error model** for actix-web, axum
and poem.

This handbook is task-oriented — it describes what the crate can do and how to
solve real problems in an application. For the API reference see
[docs.rs](https://docs.rs/treat) (or `cargo doc --open`).

## Contents

1. [Getting started](getting-started.md) — install, features, your first handler.
2. [Errors](errors.md) — the error model: codes, source chains, verbose mode, tracking.
3. [Responses](responses.md) — the envelope, `meta`, pagination, reading responses.
4. [Derive macros](derives.md) — `ApiError`, `ApiErrorCode`, `FromErrorMessage`.
5. [Framework integration](frameworks.md) — actix-web, axum, poem, request extractors, and observability middleware.
6. [Cookbook](cookbook.md) — tricks & problem → solution recipes.
7. [Troubleshooting](troubleshooting.md) — common errors and how to fix them.

## The one-minute version

```rust
use treat::prelude::*;

async fn get_user(id: u64) -> Result<ApiResponse<User>, ApiError> {
    let user = db::find(id).ok_or_api_error("user_not_found")?;   // Option -> ApiError
    let user = user.load_profile().wrap_api_error("profile_unavailable")?; // Result -> ApiError
    Ok(success(user))
}
```

- **Success** → `{ "data": ... }` (plus optional `meta`).
- **Failure** → `{ "errors": [ { "code": "...", "message": "...", "meta": ... } ] }`.
- Errors carry a machine `code`, a human `message`, arbitrary `meta`, a full
  **cause chain**, and the source location where they were raised.
- actix-web, axum and poem serialize the same error **identically**.

## Why not just `thiserror` + `IntoResponse`?

That pattern is fine, but you re-implement the envelope, the error-to-JSON
mapping and the observability glue in every service. `treat` gives you one
envelope, typed codes with derives, request extractors, and drop-in actix/tower
middleware for request ids, tracing root spans, and OpenTelemetry. See the
[cookbook](cookbook.md) for what that buys you day to day.
