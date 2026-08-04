# Derive macros

Three derives (behind the default `derive` feature) remove the boilerplate of
mapping enums to and from the wire format.

## `ApiErrorCode` — typed error codes

Turn an enum into a code usable as `ApiError<YourEnum>`. You get exhaustiveness
(the compiler lists your error space) and interpolated messages.

```rust
use treat::prelude::*;

#[derive(Clone, Debug, PartialEq, ApiErrorCode)]
enum OrderError {
    // wire code defaults to the variant name: "NotFound"
    #[message("order {id} not found")]
    NotFound { id: u64 },

    // override the wire code:
    #[code("order.already_paid")]
    AlreadyPaid,

    // tuple fields are named arg_0, arg_1, ...
    #[message("line {arg_0} exceeds limit {arg_1}")]
    OverLimit(u32, u32),
}

let err: ApiError<OrderError> = OrderError::NotFound { id: 7 }.into();
assert_eq!(err.to_error_message().code, "NotFound");
assert_eq!(err.to_error_message().message.as_deref(), Some("order 7 not found"));
```

Generated: `Display` (the code), `From<YourEnum> for ApiError<YourEnum>`, and
`From<YourEnum> for Result<ApiResponse<T, M>, YourEnum>`.

> **Set `#[code(...)]` explicitly.** The default wire code is the variant name
> verbatim — `NotFound`, not `not_found` — which clashes with the `snake_case`
> codes used everywhere else in this library and in most APIs. The default exists
> for backwards compatibility and is _not_ a recommendation: spell the wire code
> out on every variant so it stays stable when the Rust variant is renamed.

**When to use:** your own service's error space, where you want typed codes and
message templates.

## `ApiError` — adapt a `thiserror` enum

Map an existing error enum onto `ApiError<&'static str>`. The variant's `Display`
becomes the message; `#[code(...)]` sets the wire code.

```rust
use treat::prelude::*;
use treat::erris;
use thiserror::Error;

#[derive(Debug, Error, ApiError)]
enum ServiceError {
    #[error("access denied")]
    #[code("forbidden")]
    Forbidden,

    // exactly one #[catch_all] tuple variant holding a Report:
    #[catch_all]
    #[error("internal error")]
    #[code("internal")]
    Internal(#[source] erris::Report),
}

// #[catch_all] also gives you From<Report>, so `?` on a report yields your enum:
fn run() -> Result<(), ServiceError> {
    Err(erris::report!("db down"))?;
    Ok(())
}
```

Generated: `From<YourEnum> for ApiError`, plus `From<erris::Report> for YourEnum`
for the `#[catch_all]` variant.

**When to use:** you already have a `thiserror` enum and want it to become an API
error without rewriting it.

## `FromErrorMessage` — parse responses into typed errors

The reverse direction, for clients. Map a wire `ErrorMessage` (or `ApiError`)
back onto your enum by `code`; `#[code("_")]` is the catch-all.

```rust
use treat::prelude::*;

#[derive(Debug, FromErrorMessage)]
enum ClientError {
    #[code("user_not_found")]
    NotFound(ErrorMessage),
    #[code("rate_limited")]
    RateLimited(ErrorMessage),
    #[code("_")]
    Other(ErrorMessage),
}

let resp: ApiResponse<String> =
    serde_json::from_str(r#"{"errors":[{"code":"rate_limited"}]}"#).expect("json");
if let Some(err) = resp.err() {
    let typed: ClientError = err.into();
    assert!(matches!(typed, ClientError::RateLimited(_)));
}
```

Generated: `From<ErrorMessage>`, `From<&ErrorMessage>`, `From<ApiError>`,
`From<&ApiError>`.

**When to use:** consuming a `treat` service and wanting to `match` on typed
errors instead of raw code strings. Pairs with `ApiResponse::inner`/`into_inner`.

## Choosing between them

| Goal                                               | Macro              |
| -------------------------------------------------- | ------------------ |
| Define my service's typed error codes              | `ApiErrorCode`     |
| Reuse an existing `thiserror` enum as an API error | `ApiError`         |
| Parse another service's errors into my enum        | `FromErrorMessage` |
