//! [`tower`] middleware for the `treat` stack, giving axum (and any other
//! stack that serves `http::Request`s through a [`tower::Service`]) the same
//! per-request observability as the actix middleware in `treat-actix`:
//!
//! - a per-request [`RequestId`] stored in the request extensions, adopted from an
//!   incoming [`X_REQUEST_ID`] when it is a UUID and echoed back on the response,
//! - a root span (`"HTTP request"`) with the actix logger's OpenTelemetry-flavoured
//!   fields (the differences are listed in [`root_span`]), including the
//!   `exception.*` of a `treat` error the response carries,
//! - OpenTelemetry parent-context propagation from request headers (feature
//!   `telemetry`).
//!
//! ```
//! use treat_tower::TraceLayer;
//! # #[cfg(feature = "_never")]
//! let app = axum::Router::new().layer(TraceLayer::new());
//! ```
//!
//! poem is not supported: its middleware wraps `poem::Endpoint`s, and its
//! tower-compat shim needs a service over `poem::Request`, while
//! [`TraceService`] serves `http::Request`. poem services get the envelope and
//! error integration of `treat` only.

mod layer;
#[cfg(feature = "telemetry")]
pub(crate) mod otel;
mod request_id;
pub mod root_span;

pub use layer::*;
pub use request_id::*;
