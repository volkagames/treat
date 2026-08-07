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
//! OpenTelemetry-flavoured one [`treat_tower::TraceLayer`] emits. Running both
//! layers puts **two spans** on every request; pick one. `TraceLayer` is still
//! what generates the `RequestId` — stack it when you want one, and this layer
//! records it into `request_id`:
//!
//! ```ignore
//! Router::new()
//!     .route("/", get(handler))
//!     .layer(LoggingLayer::new());
//! ```
//!
//! [`treat_tower::TraceLayer`]: https://docs.rs/treat-tower

pub mod log_event_dynamic;
mod logging;
mod root_span;

pub use logging::*;
pub use root_span::*;
