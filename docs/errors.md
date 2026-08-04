# Errors

`ApiError<C>` is the heart of the crate. It carries:

| Part                      | Type                            | Purpose                                  |
| ------------------------- | ------------------------------- | ---------------------------------------- |
| `code`                    | `C` (`&'static str` by default) | machine-readable, what clients branch on |
| `message`                 | `Option<Cow<str>>`              | human-readable detail                    |
| `meta`                    | `Option<serde_json::Value>`     | arbitrary structured context             |
| `source`                  | `Option<erris::Report>`         | the underlying cause chain               |
| `location`                | `&'static Location`             | where it was raised (`#[track_caller]`)  |
| `spantrace` / `backtrace` | optional                        | captured under the matching feature      |

## Creating errors

```rust
use treat::prelude::*;
use treat::erris;

let err = treat::error("payment_failed")
    .with_message("the card was declined")
    .with_meta(serde_json::json!({ "card_last4": "4242" }))
    .with_error(erris::report!("gateway timeout"));
```

Every constructor and builder is `#[track_caller]`, so `location` points at _your_
call site, not inside the library.

- `error(code)` — bare error.
- `error_and_message(code, msg)` — code + message.
- `wrap_error(report, code, msg)` — code + message wrapping an existing report.

Builders (chainable, take `self`): `with_message`, `with_meta`, `with_verbose`,
`with_source`, `with_error` (alias for `with_source` that accepts anything
`Into<Report>`), `track`, plus the locators below (`with_pointer`,
`with_parameter`, `with_header`, `with_type`, `with_instance`).

Accessors (read-only): `code()`, `message()`, `meta()`, `source()`, `is_verbose()`,
`error_source()`, `type_uri()`, `instance()`.

> Fields are private on purpose. Mutate through `with_*`, read through the
> accessors — this keeps `location` and the source chain tamper-proof.

## Locators: pointing at the offending field

Tell the client _where_ the error is, JSON:API-style, with a `source` object —
handy for validation errors:

```rust
use treat::prelude::*;

let err = treat::error("invalid")
    .with_message("must be a valid email")
    .with_pointer("/data/attributes/email"); // JSON Pointer into the request body

let msg = err.to_error_message();
assert_eq!(msg.source.and_then(|s| s.pointer).as_deref(), Some("/data/attributes/email"));
```

- `with_pointer(p)` — JSON Pointer (RFC 6901) into the request body.
- `with_parameter(name)` — the offending query parameter.
- `with_header(name)` — the offending request header.

On the wire the empty locator is omitted; a set one serializes as
`"source": { "pointer": "/data/attributes/email" }`.

### RFC 9457 `type` and `instance`

For Problem-Details interop you can also attach a stable documentation URI and a
per-occurrence id (often the request id):

```rust
use treat::prelude::*;

let err = treat::error("rate_limited")
    .with_type("https://errors.example.com/rate-limited") // link to docs
    .with_instance("req-01H..");                          // this occurrence
```

They serialize as the `type` and `instance` members of the error object. Keep the
`code → type` mapping on your side — the library does not hardcode a catalog.

## Turning library errors into API errors

You rarely build errors by hand — you convert them at the `?`. Pick the extension
that matches what you have:

### `Option<T>` → `OkOrError`

```rust
use treat::prelude::*;
# fn demo(cache: std::collections::HashMap<u64, String>, id: u64) -> Result<String, ApiError> {
let value = cache.get(&id).cloned().ok_or_api_error("cache_miss")?;
# Ok(value) }
```

`bool` implements it too, which makes guard clauses one-liners:

```rust
use treat::prelude::*;
# fn demo(is_admin: bool) -> Result<(), ApiError> {
is_admin.ok_or_api_error("forbidden")?;
# Ok(()) }
```

### `Result<T, E: std::error::Error>` → `WrapApiError`

The foreign error is preserved as the `source`, so it still appears in logs and
verbose output.

```rust
use treat::prelude::*;
# fn demo(raw: &str) -> Result<u16, ApiError> {
let port = raw.parse::<u16>().wrap_api_error("bad_port")?;
// or attach a message:
let port = raw.parse::<u16>().wrap_api_error_and_message("bad_port", "must be a number")?;
// or compute code/message lazily:
let port = raw.parse::<u16>().wrap_api_error_with(|| ("bad_port", format!("got {raw:?}")))?;
# Ok(port) }
```

### `erris::Report` → `WithErrorCode`

```rust
use treat::prelude::*;
use treat::erris;

let err = erris::report!("disk full").with_error_code("storage_error");
```

## The source chain

`with_source` / `with_error` accumulate causes. `wrap_api_error` stores the
foreign error as the source. You can build arbitrarily deep chains mixing
`ApiError`s and plain reports:

```rust
use treat::prelude::*;
use treat::erris;

let err = treat::error("checkout_failed")
    .with_error(erris::report!("charge declined"))
    .with_error(treat::error("insufficient_funds"));

// collect_messages walks the chain and pulls out every ApiError code:
let chain = err.collect_messages();
assert!(chain.iter().any(|m| m.code == "insufficient_funds"));
```

## Verbose mode: what reaches the client

By default a response exposes only the **top-level** error — internal causes stay
in your logs, not the client payload. This is the safe default.

- Per error: `err.with_verbose()` includes the full chain in that response.
- Globally: the `verbose-error` feature forces verbose everywhere (useful in
  staging, never in production if causes may leak internals).

```rust
use treat::prelude::*;
use treat::erris;

let err = treat::error("db_unavailable").with_error(erris::report!("connection refused"));

let brief = err.into_api_response::<()>();          // 1 error: db_unavailable
assert_eq!(brief.first_error_code(), Some("db_unavailable"));
```

`into_api_response` is the single builder the actix-web, axum and poem adapters
use, so the wire format is identical across frameworks.

## Tracking (better debug trails)

`track()` (on an error) and `track_api_error()` (on a `Result`) append the current
location to the error as it bubbles up, so verbose / `Debug` output shows the full
path it travelled:

```rust
use treat::prelude::*;
# fn inner() -> Result<(), ApiError> { Err(treat::error("boom")) }
fn outer() -> Result<(), ApiError> {
    inner().track_api_error()?; // adds this frame
    Ok(())
}
```

## HTTP status

By design, errors are returned with **HTTP 200** and the failure lives in
`errors[]`. Clients branch on `code`, not on the status line. Set a real status
per error with `.with_status(404)`, or seed it from the code via the
`ApiErrorStatus` trait and `.with_code_status()` — see
[frameworks.md](frameworks.md).

### Defaulting errors to 500

The `error-status-500` feature changes the default for errors that never set a
status: they report **500** instead of 200. Useful when clients or monitoring
only look at the status line, so an unhandled failure does not read as success.

```toml
treat = { version = "*", features = ["error-status-500"] }
```

Two things it does _not_ change:

- **An explicit status still wins.** `.with_status(404)` and `.with_code_status()`
  are untouched; only the unset case moves.
- **The response body is identical.** The status is transport-only — the failure
  still travels in `errors[]`.

Because this is a Cargo feature it is crate-global and additive: if anything in
the dependency graph enables it, every dependant gets `500` defaults. Prefer
per-error `.with_status(...)` when only some paths should change.

### `X-RPC-Status` header

The `rpc-status-header` feature adds an out-of-band operation result to every
framework response:

- `X-RPC-Status: ok` for an `ApiResponse` whose `errors[]` is empty.
- `X-RPC-Status: error` for an `ApiError` or an `ApiResponse` with errors.

The header is keyed on the envelope, not the HTTP status line. It still says
`error` when an error travels with the default `200 OK`, and it still says `ok`
for a successful envelope with a non-200 status set elsewhere.
