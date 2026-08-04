//! Out-of-band success/failure signal, modelled on gRPC's `grpc-status`.
//!
//! By design a `treat` failure travels *inside* the envelope (`errors[]`), and by
//! default with `200 OK` (see [`DEFAULT_ERROR_STATUS`](crate::DEFAULT_ERROR_STATUS)).
//! That makes the status line useless as an error signal: a caller has to parse
//! the body to learn whether the call succeeded. gRPC has the same problem — the
//! HTTP layer always says `200` — and solves it with a trailer. This is the same
//! idea in a response header.
//!
//! Enabled by the `rpc-status-header` feature. Every response the crate builds
//! then carries exactly one [`X_RPC_STATUS`] header:
//!
//! | Response | Header |
//! |---|---|
//! | `ApiError` (any adapter) | `X-RPC-Status: error` |
//! | `ApiResponse` with non-empty `errors[]` | `X-RPC-Status: error` |
//! | `ApiResponse` with empty `errors[]` | `X-RPC-Status: ok` |
//!
//! The invariant is deliberately tied to `errors[]`, not to the HTTP status, so
//! it holds whatever `error-status-500` or an explicit
//! [`with_status`](crate::ApiError::with_status) does to the status line.
//!
//! Emitting `ok` as well as `error` is what makes the signal trustworthy: a
//! client can distinguish "the call succeeded" from "a proxy stripped the
//! header", which a presence-only convention cannot express.

/// Header carrying the [`OK`] / [`ERROR`] outcome.
///
/// Lowercase because HTTP/2 requires lowercase field names on the wire; HTTP/1.1
/// field names are case-insensitive, so `X-RPC-Status` matches this.
pub const X_RPC_STATUS: &str = "x-rpc-status";

/// [`X_RPC_STATUS`] value for a response whose `errors[]` is empty.
pub const OK: &str = "ok";

/// [`X_RPC_STATUS`] value for a response carrying at least one error.
pub const ERROR: &str = "error";
