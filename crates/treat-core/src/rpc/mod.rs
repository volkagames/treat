//! The framework-free part of the RPC runtime: what generated RPC code (such as
//! `graphql-rpcgen`'s) needs from `treat` without pulling in a server or a
//! client.
//!
//! - [`WireEnum`] — an enum whose values travel by wire name; derive it with
//!   `#[derive(treat::rpc::WireEnum)]`.
//! - [`RuntimeCodes`] — the registry codes the runtime reports on its own.
//! - [`Narrow`] — report a flat [`Failure`] through the set of codes an operation
//!   declares; [`Undeclared`] names what such a set answers with for any other.
//! - [`route`] — find the row of an operation table the router would pick for a
//!   request.
//! - [`HttpSpec`] — how an HTTP-bound operation sits on the wire: its
//!   [`HttpMethod`], path, field table, [`HttpBodyKind`] and envelope.
//! - `validate` (feature `validator`) — `@pattern`-style checks.
//!
//! These live in `treat-core` rather than next to the axum/reqwest runtime in
//! `treat-rpc` because the derive macros resolve their paths to `treat` or
//! `treat-core`, and none of these items needs a dependency the core does not
//! already have. The facade merges both crates into one `treat::rpc`.
//!
//! The runtime's helpers that need the registry type — and would otherwise be
//! called with a turbofish — are inherent functions of `treat::rpc::Runtime<R>`,
//! which is defined in `treat-rpc`, not here: an inherent `impl` must live in
//! the crate that defines the type, and those helpers are the axum/reqwest
//! code. Defining the marker here would force them into extension traits that
//! every caller has to import.
//!
//! Every item is written to the strict lint set below: the runtime runs inside
//! services that deny these lints themselves, and a panic in a request path is
//! an outage rather than an error response.
#![deny(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::string_slice,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable,
    clippy::unwrap_used
)]

mod codes;
mod http;
mod query;
pub mod route;
#[cfg(feature = "validator")]
pub mod validate;

pub use codes::*;
pub use http::*;
pub use query::*;
