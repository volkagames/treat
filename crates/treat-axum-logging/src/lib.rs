//! Error-logging layer for axum.
//!
//! Once the response is built, logs any `treat` error it carries. The error is
//! read back from the response extensions — `treat`'s axum adapter stashes it
//! there as a type-erased [`treat_core::ApiErrorHandler`], which is what lets
//! this layer log a typed error without knowing the service's error-code enum.
//!
//! The layer also opens a [root span](root_span_on_request_start) in `call`,
//! before the inner service runs, so the whole request is tracked and everything
//! logged under it — including the error at the end — carries the request's
//! fields.
//!
//! That span uses a compact `request_*` / `http_*` field set, distinct from the
//! OpenTelemetry-flavoured one [`treat_tower::TraceLayer`] emits. This layer
//! never generates a `RequestId`: `request_id` is recorded only when
//! `TraceLayer` has already put one in the request extensions, so it has to be
//! the **outer** layer (in axum the last `.layer` call is the outermost):
//!
//! ```ignore
//! Router::new()
//!     .route("/", get(handler))
//!     .layer(LoggingLayer::new())   // inner: logs the error, records request_id
//!     .layer(TraceLayer::new());    // outer: resolves the RequestId first
//! ```
//!
//! The trade-off: with both layers every request gets **two nested spans**
//! (`HTTP request` from `TraceLayer`, `request` from this layer), and the
//! response gets `x-request-id`. With this layer alone there is one span, but
//! no request id anywhere — not in the span, not on the response.
//!
//! `http_client_ip` is the raw `X-Forwarded-For` header as the client sent it
//! (every hop, unparsed): it is spoofable and is only as trustworthy as the
//! proxy in front of the service. `request_uri` is the full URI, query string
//! included, so tokens passed in the query end up in the logs.
//!
//! [`treat_tower::TraceLayer`]: https://docs.rs/treat-tower

pub mod log_event_dynamic;
mod logging;
mod root_span;

pub use logging::*;
pub use root_span::*;

#[doc(hidden)]
pub mod __private {
    //! Paths for the code `event_dynamic_lvl!` expands to; not public API.
    pub use tracing;
}
