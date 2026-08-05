//! actix-web middleware for the `treat` stack: a tracing [`Logger`] with a
//! pluggable [`RootSpanBuilder`], per-request [`RequestId`] (adopted from an
//! incoming [`X_REQUEST_ID`] when it is a UUID and echoed back on the response), a
//! [`RootSpan`] extractor, and (under the `telemetry` feature) OpenTelemetry
//! parent-context propagation. Depends on [`treat_core`] for the `ApiError`
//! response type.
//!
//! # Attribution
//!
//! This crate is a trimmed-down, adapted derivative of
//! [`tracing-actix-web`](https://github.com/LukeMathWalker/tracing-actix-web)
//! by Luca Palmieri, used under the MIT License. See the `NOTICE` file shipped
//! with this crate for the full notice.

mod logger;
#[cfg(feature = "telemetry")]
pub mod otel;
mod request_id;
mod root_span;
mod root_span_builder;
pub mod root_span_macro;

pub use logger::*;
pub use request_id::*;
pub use root_span::*;
pub use root_span_builder::*;
pub use root_span_macro::*;
