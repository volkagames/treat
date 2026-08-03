# Responses

`ApiResponse<T, M>` is the envelope. `T` is the `data` type, `M` is the `meta`
type (both default to `()`):

```json
{ "data": { ... }, "meta": { ... }, "errors": [ ... ] }
```

The envelope enforces two invariants on the wire:

- `errors` is either absent or a non-empty array — never `"errors": []`.
- a success (no errors) always carries `data`, even as `"data": null`; an error
  response omits `data` entirely, so `data` and `errors` never coexist.

So a plain success is `{"data": ...}` (or `{"data": null}`), a `meta` slot is added
when set, and a plain failure is `{"errors":[...]}`.

## Building responses

```rust
use leto::prelude::*;

// success: data only
let ok = success(vec![1, 2, 3]);

// failure: errors only (from an ErrorMessage, an ApiError, or an iterator)
let bad: ApiResponse<()> = leto::error("nope").into();

// builders (chainable):
let full = ApiResponse::<i32, String>::from(1)
    .with_data(2)
    .with_meta("page-1".to_string())
    .with_errors([/* ErrorMessage, ... */]);
```

`From` conversions you get for free:

- `T` → `ApiResponse<T, M>` (data)
- `ApiError` / `&ApiError` → `ApiResponse` (errors)
- `Result<T, ApiError>` → `ApiResponse<T>` (data or errors)

That last one is why handlers can `return result.into()`.

## `meta`: pagination, counts, anything

`meta` is a typed slot — use a struct for structured metadata:

```rust
use leto::prelude::*;
use serde::Serialize;

#[derive(Serialize)]
struct Page { total: u64, next: Option<String> }

let resp: ApiResponse<Vec<u8>, Page> = ApiResponse::from(vec![1u8, 2, 3])
    .with_meta(Page { total: 128, next: Some("cursor123".into()) });
```

### Ready-made slots (`meta-slots` feature)

Common shapes come built in, so you don't have to redefine them. `Pagination`
derives `total_pages` for you; `RateLimit` mirrors the `RateLimit-*` headers.
Attach a typed meta with `success_with_meta` (`success` alone fixes the meta type
to `NoMeta`):

```rust,ignore
use leto::{success_with_meta, meta_slots::Pagination};

let page = Pagination::new(2, 20, 137); // page 2, 20/page, 137 total → total_pages = 7
let resp = success_with_meta(users, page);
// => { "data": [...], "meta": { "page": 2, "per_page": 20, "total": 137, "total_pages": 7 } }
```

## Reading a response (client side)

When you're the _consumer_ of a `leto` service, deserialize into
`ApiResponse<T>` and inspect it:

```rust
use leto::prelude::*;

let resp: ApiResponse<String> =
    serde_json::from_str(r#"{"errors":[{"code":"user_not_found"}]}"#).expect("json");

// convenience accessors:
assert!(resp.ok().is_none());                       // Option<&T>
assert_eq!(resp.err().map(|e| e.code.as_str()), Some("user_not_found")); // Option<&ErrorMessage>
assert_eq!(resp.first_error_code(), Some("user_not_found"));
assert!(resp.has_error_code("user_not_found").is_some());
```

To collapse a response into a `Result` in your own code, use the `inner*`
extractors:

- `inner::<E>()` / `into_inner::<E>()` — `Ok(Option<&T>)` or `Err(E)`, where `E:
From<&ErrorMessage> + Into<erris::Report>` (pair this with
  [`FromErrorMessage`](derives.md) for typed client errors).
- `inner_data()` / `into_inner_data()` — `Ok(T)` or an `erris::Report` describing
  the first error / the missing `data`.

```rust
use leto::prelude::*;

let resp: ApiResponse<i32> = success(42);
assert_eq!(resp.inner_data().expect("data"), &42);
```

## Out-of-band result header

Enable `rpc-status-header` when a client, gateway, or metric pipeline needs to
know the operation result before parsing the body. Framework adapters then add:

- `X-RPC-Status: ok` when the envelope has no errors.
- `X-RPC-Status: error` when the envelope has at least one error.

The HTTP status remains transport-only. The header mirrors `errors[]`.

## OpenAPI

With the `openapi` feature, `ResponseData` additionally requires
`utoipa::ToSchema`, so your `data` / `meta` types need `#[derive(utoipa::ToSchema)]`.

The envelope itself derives `ToSchema` under this feature, so you can name it
directly in `#[utoipa::path]` responses:

```rust,ignore
#[utoipa::path(
    get, path = "/users/{id}",
    responses(
        (status = 200, body = ApiResponse<User>),  // success envelope
        (status = 400, body = ErrorResponse),        // { "errors": [ ... ] }
    ),
)]
async fn get_user(/* ... */) { /* ... */ }
```

- `ApiResponse<T, M>` — the full `{ data, meta, errors }` schema.
- `ErrorResponse` — the error-only shape (`{ "errors": [ErrorMessage] }`), a
  convenient `body` for failure responses. It serializes identically to an
  `ApiResponse` that carries only errors.
- `ErrorMessage` / `ErrorSource` — the error object and its locator also expose
  schemas.
