# Derive macros

Three derives (behind the default `derive` feature) remove the boilerplate of
mapping enums to and from the wire format.

A fourth, `#[derive(treat::rpc::WireEnum)]`, gives a `Copy` enum of unit
variants its wire names for the RPC runtime — `#[code]`, else serde's name,
else the identifier. `as_str`, `ApiErrorCode`'s `Display` and serde agree
wherever serde is told a name: the derive refuses a `#[code]` that a serde
`rename` / `rename_all` spells differently. A `#[code]` serde is not told about
is the wire name while serde still writes the identifier — add a matching
`#[serde(rename)]` if the enum also travels as a payload. See
[rpc.md](rpc.md#codes-by-wire-name).

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

    // a transport HTTP status for this variant:
    #[code("order.locked")]
    #[status(409)]
    Locked,
}

let err: ApiError<OrderError> = OrderError::NotFound { id: 7 }.into();
assert_eq!(err.to_error_message().code, "NotFound");
assert_eq!(err.to_error_message().message.as_deref(), Some("order 7 not found"));

let err: ApiError<OrderError> = OrderError::Locked.into();
assert_eq!(err.status(), 409);
```

Generated: `Display` (the code), `From<YourEnum> for ApiError<YourEnum>`, and
`From<YourEnum> for Result<ApiResponse<T, M>, YourEnum>`.

A `#[message]` binds only the fields it reads, so a variant whose message skips
a field compiles without an `unused_variables` warning. A field that neither the
message nor any other code reads is then reported by rustc's `dead_code` lint
("field `b` is never read") when the enum is not public (a binary crate,
`pub(crate)`); a derived `Debug` does not count as a read. That warning is
genuine: read the field somewhere, drop it, or put `#[allow(dead_code)]` on that
field.

**`#[status(N)]`** sets the transport HTTP status of a variant (`100..=599`, a
value outside fails the build). The generated `From` applies it with
`with_status`, so `.into()` and every `From`-based conversion
(`ok_or_api_code`, `wrap_api_code`, `wrap_api_error_default`, `with_api_code`)
carry it. As soon as one variant has `#[status]`, the derive also implements
`ApiErrorStatus` for the enum, so `treat::error(code).with_code_status()` works
too; a variant without the attribute maps to `DEFAULT_ERROR_STATUS` (`200`, or
`500` under `error-status-500`). With no `#[status]` anywhere, no
`ApiErrorStatus` impl is generated and you may write your own.

```rust
use treat::prelude::*;
use treat::ApiErrorStatus;

#[derive(Clone, Debug, ApiErrorCode)]
enum Code {
    #[code("not_found")]
    #[status(404)]
    NotFound,
    #[code("busy")]
    Busy,
}

assert_eq!(Code::NotFound.status_code(), 404);
assert_eq!(Code::Busy.status_code(), treat::DEFAULT_ERROR_STATUS);
assert_eq!(treat::error(Code::NotFound).with_code_status().status(), 404);
```

**Generic enums** are supported. The impls carry the enum's parameters and
bounds; the conversion into `ApiError<YourEnum<..>>` exists where
`YourEnum<..>` is a `'static` `ApiErrorCode` (`Debug + Clone + Display + Send +
Sync`). Put the bounds a message needs (`T: Display`) on the enum itself.

> **Set `#[code(...)]` explicitly.** Without it the wire code is the name serde
> writes for the variant (`#[serde(rename)]`, the enum's `#[serde(rename_all)]`),
> and without those the variant name
> verbatim — `NotFound`, not `not_found`; a raw identifier without its prefix,
> `r#type` as `type` — which clashes with the `snake_case`
> codes used everywhere else in this library and in most APIs. The default exists
> for backwards compatibility and is _not_ a recommendation: spell the wire code
> out on every variant so it stays stable when the Rust variant is renamed.
>
> **Changed in 0.26**, both changing the code a variant without `#[code]` is
> sent as:
>
> - `Display` follows serde's `rename` and `rename_all`, as
>   `#[derive(WireEnum)]` does, so the code in an envelope is the value a JSON
>   payload carries. Under `#[serde(rename_all = "snake_case")]`, `NotFound`
>   used to be sent as `NotFound` and is now `not_found`.
> - A raw identifier lost its prefix: `r#type` used to be sent as `r#type` and
>   is now `type`, the name serde writes.
>
> A variant whose clients still match the old code keeps it with `#[code]`:
> `#[code("NotFound")]`, `#[code("r#type")]`.

**When to use:** your own service's error space, where you want typed codes and
message templates.

## `ApiError` — adapt a `thiserror` enum

Map an existing error enum onto `ApiError<&'static str>`. The variant's `Display`
becomes the message; `#[code(...)]` sets the wire code and is required on every
variant: one without it fails the build with an error naming the variant.
`#[status(N)]` sets the variant's transport status, as for `ApiErrorCode`.

```rust
use treat::prelude::*;
use treat::erris;
use thiserror::Error;

#[derive(Debug, Error, ApiError)]
enum ServiceError {
    #[error("access denied")]
    #[code("forbidden")]
    #[status(403)]
    Forbidden,

    // at most one #[catch_all]: a tuple variant with exactly one field, a Report
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
for the `#[catch_all]` variant. The catch-all is optional; a second one, or one
that is not a single-field tuple variant, fails the build.

A generic enum is supported; the conversion requires `YourEnum<..>: Display`,
so it exists wherever the `thiserror` impl does.

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

// or collapse the whole response into a Result in one call:
let typed = resp.inner::<ClientError>().expect_err("an error response");
assert!(matches!(typed, ClientError::RateLimited(_)));
```

Generated: `From<ErrorMessage>`, `From<&ErrorMessage>`, `From<ApiError>`,
`From<&ApiError>`.

Every variant needs a `#[code]` and holds exactly one field (the
`ErrorMessage`, or a type it converts into); exactly one variant is the
`#[code("_")]` catch-all, and a concrete code may be claimed by one variant
only — a duplicate fails the build instead of becoming an unreachable arm. A
generic enum is supported; each field type must then be `From<ErrorMessage>`.

**When to use:** consuming a `treat` service and wanting to `match` on typed
errors instead of raw code strings. Pairs with `ApiResponse::inner` /
`into_inner`, which need only the generated `From` impls. To also send the enum
through `?` into an `erris::Result`, give it `Display` and
`std::error::Error` (e.g. `#[derive(thiserror::Error)]`).

## Choosing between them

| Goal                                               | Macro              |
| -------------------------------------------------- | ------------------ |
| Define my service's typed error codes              | `ApiErrorCode`     |
| Reuse an existing `thiserror` enum as an API error | `ApiError`         |
| Parse another service's errors into my enum        | `FromErrorMessage` |
