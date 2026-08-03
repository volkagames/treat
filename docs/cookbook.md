# Cookbook

Problem → solution recipes for everyday application code.

## Guard clauses without an `if`

`bool` implements `OkOrError`, so a permission check is one line:

```rust
use leto::prelude::*;
# fn demo(is_owner: bool) -> Result<(), ApiError> {
is_owner.ok_or_api_error("forbidden")?;
# Ok(()) }
```

## Convert any `?` into a coded error

Chain conversions from different error kinds in one function — each keeps its
original error as the source:

```rust
use leto::prelude::*;
# fn load(raw: Option<&str>) -> Result<u16, ApiError> {
let raw = raw.ok_or_api_error("missing_port")?;              // Option
let port = raw.parse::<u16>().wrap_api_error("bad_port")?;   // Result<_, ParseIntError>
# Ok(port) }
```

## Attach structured detail for the client

Put machine-usable context in `meta` (validation fields, retry hints, ...):

```rust
use leto::prelude::*;

let err = leto::error("validation_failed")
    .with_message("check the highlighted fields")
    .with_meta(serde_json::json!({
        "fields": { "email": "must be a valid address", "age": "must be >= 18" }
    }));
```

## Return data _and_ metadata

```rust
use leto::prelude::*;
# #[derive(serde::Serialize)] struct Meta { total: u64 }
# fn page() -> ApiResponse<Vec<u8>, Meta> {
ApiResponse::from(vec![1u8, 2, 3]).with_meta(Meta { total: 100 })
# }
```

## Add breadcrumbs as an error bubbles up

`track_api_error()` records each layer it passes through, so verbose/`Debug`
output shows the full path:

```rust
use leto::prelude::*;
# fn repo() -> Result<(), ApiError> { Err(leto::error("db_timeout")) }
fn service() -> Result<(), ApiError> {
    repo().track_api_error()?;
    Ok(())
}
```

## Typed error space for your service

Define codes once, get exhaustiveness and message templates (see
[derives](derives.md)):

```rust
use leto::prelude::*;

#[derive(Clone, Debug, PartialEq, ApiErrorCode)]
enum AuthError {
    #[message("token expired {secs}s ago")]
    Expired { secs: u64 },
    InvalidCredentials,
}

fn refresh(secs: u64) -> Result<ApiResponse<()>, AuthError> {
    Err(AuthError::Expired { secs })
}
```

## Consume another service's errors as your own enum

```rust
use leto::prelude::*;

#[derive(Debug, FromErrorMessage)]
enum Upstream {
    #[code("rate_limited")]
    RateLimited(ErrorMessage),
    #[code("_")]
    Other(ErrorMessage),
}

# fn handle(resp: ApiResponse<String>) {
if let Some(err) = resp.err() {
    match Upstream::from(err) {
        Upstream::RateLimited(_) => { /* back off and retry */ }
        Upstream::Other(_) => { /* surface to caller */ }
    }
}
# }
```

## Decide how much detail leaks

- Production: keep the default — clients see only the top-level error.
- Staging: enable the `verbose-error` feature (or call `.with_verbose()`) to send
  the whole cause chain for debugging.

## Don't pull in `erris` separately

Reach the report type through the re-export:

```rust
use leto::erris; // instead of adding erris to Cargo.toml
let report = erris::report!("something went wrong");
```

## Point at the field that failed (validation → locators)

The field-level locators (`source.pointer`) are most useful when something fills
them in for you.

Malformed request bodies — enable `serde-path` and parse with
`deserialize_body`, which reports the offending field as a JSON Pointer:

```rust,ignore
use leto::deserialize_body;
let body: CreateUser = deserialize_body(&bytes)?; // Err carries pointer "/email"
```

Field validation — enable `validator`, derive `Validate`, and call
`validate_api()`; each violation becomes one `errors[]` entry with its pointer:

```rust,ignore
use leto::ValidateApi;
body.validate_api()?; // Err is an ApiResponse with one error per invalid field
```

In a handler, enable `validator-extract` (+ `axum` or `actix`) and let the
`ApiValidated<T>` extractor parse **and** validate in one step. It yields a
`Validated<T>` — a type that can only exist after validation passed, so there is
nothing to re-validate downstream:

```rust,ignore
use leto::extract_axum::ApiValidated;
async fn create(ApiValidated(body): ApiValidated<CreateUser>) -> ApiResponse<User> {
    // `body: Validated<CreateUser>` — already valid. Access via Deref or `.into_inner()`.
    success(insert(body.into_inner()))
}
```

Bad JSON returns `invalid_body` with a pointer; an invalid field returns the
validator rule code (`email`, `length`, …) with a pointer — no raw `400`.
