//! Runtime for generated RPC services on `treat`: the code a generator (such as
//! `graphql-rpcgen`) would otherwise copy into every project it emits.
//!
//! Use it through the `treat` facade's `rpc-server`, `rpc-client` and
//! `rpc-stream` features, as `treat::rpc`. That one namespace holds this
//! crate's items and the framework-free ones from `treat-core`'s `rpc` module
//! ([`WireEnum`], [`RuntimeCodes`], [`Narrow`], [`route`], [`QueryField`],
//! ...), which are re-exported here as well.
//!
//! | Feature | Module | Enables |
//! |---------|--------|---------|
//! | `server` | `server` | the axum server runtime |
//! | `client` | `client` | the reqwest client runtime, without TLS |
//! | `stream` | — | SSE and raw byte bodies for the above; re-exports `bytes`, `futures_core`, `futures_util` |
//! | `rustls` | — | `https` for the client: `reqwest`'s `rustls` feature |
//! | `rpc-status-header` | — | `x-rpc-status: ok` on bare (`@http(envelope: false)`), raw and SSE successes too |
//!
//! Helpers that need the generated registry are inherent functions of
//! [`Runtime<R>`], so generated code names the registry once —
//! `type Rt = treat::rpc::Runtime<ErrorCode>;` — and calls `Rt::helper(..)`
//! without a turbofish.
//!
//! The crate denies the lints below: it runs inside services that deny them
//! themselves, and a panic in a request path is an outage rather than an error
//! response.
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

// `stream` turns `http` on for the client, which needs it only together with
// `stream` — a pair of features Cargo cannot name as one dependency's
// condition. Without `client` nothing reads it; this keeps that build quiet.
#[cfg(all(feature = "stream", not(feature = "client")))]
use http as _;
use std::marker::PhantomData;
pub use treat_core::rpc::*;
#[cfg(feature = "stream")]
pub use {bytes, futures_core, futures_util};

#[cfg(feature = "client")]
pub mod client;
// The server reads `Content-Type` for every body; the client only for the
// answers `stream` adds, a raw one or an event stream.
#[cfg(any(feature = "server", all(feature = "client", feature = "stream")))]
mod media;
#[cfg(feature = "server")]
pub mod server;

/// The runtime's helpers for the registry `R`, as associated functions.
///
/// A namespace, not a value: it is never constructed. A helper that needs the
/// registry — to build a failure with [`RuntimeCodes::INVALID_BODY`], say —
/// would otherwise be a generic function its every caller turbofishes;
/// generated code instead aliases the marker once,
/// `type Rt = treat::rpc::Runtime<ErrorCode>;`, and calls `Rt::helper(..)`.
///
/// Defined here rather than in `treat-core` with the traits because an inherent
/// `impl` must live in the crate that defines the type, and the helpers are
/// this crate's axum and reqwest code: each module adds its own
/// `impl<R: RuntimeCodes> Runtime<R>` block.
pub struct Runtime<R>(PhantomData<fn() -> R>);
