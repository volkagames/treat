//! A lightweight HTTP response **envelope** (`{ data, meta, errors[] }`) with a
//! rich, **typed error model** for actix-web and axum services.
//!
//! `leto` gives you one response shape for success *and* failure, errors that
//! carry a machine `code` + human `message` + `meta` + a full source chain (via
//! [`erris`]) + the `#[track_caller]` location, ergonomic `?`-conversions, derive
//! macros, and drop-in actix middleware (request-id, tracing, OpenTelemetry).
//!
//! # Quick start
//!
//! A handler returns [`ApiResponse<T>`](ApiResponse) on success or an
//! [`struct@ApiError`] on failure; `?` does the conversion:
//!
//! ```
//! use leto::prelude::*;
//!
//! fn get_user(id: u64) -> Result<ApiResponse<String>, ApiError> {
//!     let name = lookup(id).ok_or_api_error("user_not_found")?;
//!     Ok(success(name))
//! }
//!
//! fn lookup(id: u64) -> Option<String> {
//!     (id == 1).then(|| "Ada".to_string())
//! }
//!
//! assert_eq!(get_user(1).expect("found").data.as_deref(), Some("Ada"));
//! assert_eq!(*get_user(2).expect_err("missing").code(), "user_not_found");
//! ```
//!
//! On the wire: success is `{"data":...}` (plus optional `meta`), failure is
//! `{"errors":[{"code":"...","message":"...","meta":...}]}`. Empty fields are
//! omitted.
//!
//! # Converting errors with `?`
//!
//! | You have | Extension | Methods |
//! |----------|-----------|---------|
//! | `Option<T>` / `bool` | [`OkOrError`] | `ok_or_api_error`, `ok_or_api_error_with_message` |
//! | `Result<T, E: Error>` | [`WrapApiError`] | `wrap_api_error`, `wrap_api_error_and_message`, `wrap_api_error_with` |
//! | an [`erris::Report`] | [`WithErrorCode`] | `with_error_code`, `with_error_code_and_message` |
//!
//! ```
//! use leto::prelude::*;
//!
//! fn parse_port(raw: &str) -> Result<ApiResponse<u16>, ApiError> {
//!     let port = raw.parse::<u16>().wrap_api_error("bad_port")?; // std error kept as source
//!     Ok(success(port))
//! }
//! # assert_eq!(*parse_port("x").expect_err("e").code(), "bad_port");
//! ```
//!
//! # Documentation
//!
//! This is the API reference. The **handbook** — feature guide, tricks and
//! problem→solution recipes — lives in the `docs/` directory of the repository:
//!
//! * `docs/getting-started.md` — install, features, first handler.
//! * `docs/errors.md` — the error model in depth (codes, chains, verbose, tracking).
//! * `docs/responses.md` — the envelope, `meta`, reading responses.
//! * `docs/derives.md` — the `ApiError` / `ApiErrorCode` / `FromErrorMessage` macros.
//! * `docs/frameworks.md` — actix-web, axum, poem and the observability middleware.
//! * `docs/cookbook.md` — everyday recipes.
//! * `docs/troubleshooting.md` — common errors and fixes.
//!
//! # Feature flags
//!
//! | Feature | Enables |
//! |---------|---------|
//! | `derive` *(default)* | the `ApiError` / `ApiErrorCode` / `FromErrorMessage` macros |
//! | `actix` | actix-web `Responder`/`ResponseError` + middleware + telemetry + [`response_get_api_error_actix`] |
//! | `actix-middleware` / `actix-telemetry` | the middleware / OpenTelemetry parts on their own |
//! | `axum` | axum `IntoResponse` + [`response_get_api_error`] |
//! | `poem` | poem `IntoResponse` + `ResponseError` + [`response_get_api_error_poem`] |
//! | `tower-middleware` / `tower-telemetry` | tower request-id + root span (+ OpenTelemetry) for axum/poem |
//! | `serde-path` | `deserialize_body`: JSON parse errors carry a field pointer |
//! | `validator` | `validate_api` / `Validated<T>`: field errors carry pointers |
//! | `validator-extract` | `ApiJson<T>` / `ApiValidated<T>` request extractors (needs `axum`/`actix`) |
//! | `openapi` | `utoipa::ToSchema` on the envelope + response data |
//! | `meta-slots` | ready-made typed `meta` payloads (`Pagination`, `RateLimit`) |
//! | `spantrace` / `backtrace` | capture a span-/back-trace on every error |
//! | `verbose-error` | always serialize the full cause chain |
//! | `nightly-provide` | `std::error::Error::provide` support (**requires nightly**) |

pub use leto_core::*;

pub mod prelude {
    pub use leto_core::prelude::*;
    // Re-export the derive macros so that `use leto::prelude::*` brings both the
    // `ApiError`/`ApiErrorCode` items (type namespace) and the matching derive macros
    // (macro namespace), as in the original single-crate layout.
    #[cfg(feature = "derive")]
    pub use leto_derive::*;
}

#[cfg(feature = "actix-middleware")]
pub use leto_actix::*;
#[cfg(feature = "derive")]
pub use leto_derive::*;
/// tower middleware (request-id + root span + otel) for axum/poem. Namespaced
/// rather than glob-re-exported so its `RequestId` does not collide with the
/// actix middleware's when both are enabled. Enable with `tower-middleware`.
#[cfg(feature = "tower-middleware")]
pub use leto_tower as tower;
