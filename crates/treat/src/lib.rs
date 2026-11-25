//! A lightweight HTTP response **envelope** (`{ data, meta, errors[] }`) with a
//! rich, **typed error model** for actix-web, axum and poem services.
//!
//! `treat` gives you one response shape for success *and* failure, errors that
//! carry a machine `code` + human `message` + `meta` + a full source chain (via
//! [`erris`]) + the `#[track_caller]` location, ergonomic `?`-conversions, derive
//! macros, request extractors that answer in the envelope, and drop-in actix-web
//! and tower (axum) middleware (request-id, tracing, OpenTelemetry).
//!
//! # Quick start
//!
//! A handler returns [`ApiResponse<T>`](ApiResponse) on success or an
//! [`struct@ApiError`] on failure; `?` does the conversion:
//!
//! ```
//! use treat::prelude::*;
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
//! On the wire: success is `{"data":...}` (plus optional `meta`), with `data`
//! always present — `"data":null` when there is none. Failure is
//! `{"errors":[{"code":"...","message":"...","meta":...}]}`, with no `data`. An
//! absent `meta`, and each optional field of an error entry it does not set
//! (`message`, `source`, `type`, `instance`, `meta`), are omitted.
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
//! use treat::prelude::*;
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
//! * `docs/derives.md` — the `ApiError` / `ApiErrorCode` / `FromErrorMessage` / `WireEnum` macros.
//! * `docs/frameworks.md` — actix-web, axum, poem, request extractors and the middleware.
//! * `docs/cookbook.md` — everyday recipes.
//! * `docs/troubleshooting.md` — common errors and fixes.
//! * `docs/rpc.md` — `treat::rpc`, the runtime generated RPC code links against.
//!
//! # Feature flags
//!
//! | Feature | Enables |
//! |---------|---------|
//! | `derive` *(default)* | the `ApiError` / `ApiErrorCode` / `FromErrorMessage` / `rpc::WireEnum` macros |
//! | `actix` | actix-web `Responder`/`ResponseError` + middleware + telemetry + [`response_get_api_error_actix`] |
//! | `actix-middleware` / `actix-telemetry` | the middleware / OpenTelemetry parts on their own |
//! | `axum` | axum `IntoResponse` + [`response_get_api_error`]; with `tracked`, an `erris::Result` handler return |
//! | `poem` | poem `IntoResponse` + `ResponseError` + [`response_get_api_error_poem`] (no middleware, no extractors) |
//! | `tower-middleware` / `tower-telemetry` | tower request-id + root span (+ OpenTelemetry) for axum: `treat::tower` |
//! | `serde-path` | `deserialize_body` (JSON errors with field pointers); `ApiJson<T>` (with `axum` / `actix`) |
//! | `validator` | `validate_api` / `Validated<T>`: field errors carry pointers; `rpc::validate::matches_pattern` |
//! | `validator-extract` | `ApiValidated<T>` (with `axum` / `actix`); implies `validator` + `serde-path` |
//! | `openapi` | `utoipa::ToSchema` on the envelope + response data |
//! | `meta-slots` | ready-made typed `meta` payloads (`Pagination`, `RateLimit`) |
//! | `spantrace` / `backtrace` | capture a span-/back-trace on every error; span traces also need `ErrorLayer` |
//! | `verbose-error` | always serialize the full cause chain |
//! | `error-status-500` | an error with no explicit status answers `500` instead of `200` |
//! | `rpc` | `rpc::Runtime` for generated RPC code; the framework-free [`rpc`] items need no feature |
//! | `rpc-server` / `rpc-client` | the axum server / reqwest client runtime in [`rpc`] (imply `rpc`) |
//! | `rpc-client-rustls` | `rpc-client` with `reqwest`'s `rustls` (aws-lc-rs, platform verifier) for `https` |
//! | `rpc-stream` | server-sent events and raw byte bodies for the RPC server / client |
//! | `rpc-status-header` | `x-rpc-status` on every envelope; with `rpc-server` on bare, raw, SSE successes too |
//! | `nightly-provide` | `std::error::Error::provide` support (**requires nightly**) |
//! | `tracked` | `ApiError` as the error of an erris `TrackedResult` (**requires nightly**; not additive) |

pub use treat_core::*;

pub mod prelude {
    pub use treat_core::prelude::*;
    // Re-export the derive macros so that `use treat::prelude::*` brings both the
    // `ApiError`/`ApiErrorCode` items (type namespace) and the matching derive macros
    // (macro namespace), as in the original single-crate layout.
    #[cfg(feature = "derive")]
    pub use treat_derive::*;
}

/// Runtime for generated RPC services (see `docs/rpc.md`).
///
/// One namespace over the crates that define its items: the framework-free
/// traits and helpers of `treat-core` (always here), the `WireEnum` derive
/// (feature `derive`), and with `rpc` / `rpc-server` / `rpc-client` /
/// `rpc-stream` the axum and reqwest runtime of `treat-rpc`. So
/// `treat::rpc::X` names an item wherever it is defined, and
/// `#[derive(treat::rpc::WireEnum)]` sits next to the trait it implements.
pub mod rpc {
    // One glob per build: `treat_rpc` re-exports `treat_core::rpc::*` itself,
    // so with `rpc` it alone covers both crates, and without it the core's
    // items are all there is.
    #[cfg(not(feature = "rpc"))]
    pub use treat_core::rpc::*;
    #[cfg(feature = "derive")]
    pub use treat_derive::WireEnum;
    #[cfg(feature = "rpc")]
    pub use treat_rpc::*;
}

#[cfg(feature = "actix-middleware")]
pub use treat_actix::*;
#[cfg(feature = "derive")]
pub use treat_derive::*;
/// tower middleware (request-id + root span + otel) for axum. Namespaced
/// rather than glob-re-exported so its `RequestId` does not collide with the
/// actix middleware's when both are enabled. Enable with `tower-middleware`.
#[cfg(feature = "tower-middleware")]
pub use treat_tower as tower;
