//! [`tower`] middleware for the `leto` stack, giving `tower`-based frameworks
//! (axum, and poem via its tower-compat layer) the same per-request
//! observability as the actix middleware in `leto-actix`:
//!
//! - a per-request [`RequestId`] stored in the request extensions,
//! - a root span (`"HTTP request"`) with the same OpenTelemetry-flavoured fields,
//! - OpenTelemetry parent-context propagation from request headers (feature
//!   `telemetry`).
//!
//! ```
//! use leto_tower::TraceLayer;
//! # #[cfg(feature = "_never")]
//! let app = axum::Router::new().layer(TraceLayer::new());
//! ```
//!
//! # poem
//!
//! poem's middleware model differs from tower's, but poem can consume a tower
//! [`Layer`](tower::Layer) via `poem::middleware::TowerLayerCompatExt`:
//!
//! ```ignore
//! use poem::middleware::TowerLayerCompatExt;
//! let app = route.with(leto_tower::TraceLayer::new().compat());
//! ```

mod layer;
#[cfg(feature = "telemetry")]
pub(crate) mod otel;
mod request_id;
pub mod root_span;

pub use layer::*;
pub use request_id::*;
