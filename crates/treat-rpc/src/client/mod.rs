//! The reqwest client runtime (feature `client`, `treat`'s `rpc-client`).
//!
//! A generated client is a newtype over [`RpcClient`] with an accessor per
//! service, and each of its methods is one call into it:
//!
//! | Operation | Request | Method |
//! |-----------|---------|--------|
//! | RPC | `POST` + JSON body | [`RpcClient::call`] |
//! | `@http` | as its [`HttpSpec`](crate::HttpSpec) says | [`RpcClient::call_http`] |
//! | `@subscription` | `GET` + query string, SSE back | `subscribe` |
//! | `@raw(response:)` | `POST` + JSON body, bytes back | `raw` |
//! | `@raw(request:)` | `POST` + bytes, input in the query string | `raw_request`, `raw_request_json` |
//!
//! The last three take feature `stream`. Every JSON body goes with
//! `Content-Type: application/json`, which the server requires of an RPC body,
//! and a raw body with the type the caller names, which must be one its
//! operation declares. An input sent in the URL — a query string, a form body,
//! a path — is spelled as the operation's [`QueryField`](crate::QueryField)
//! table says, the same table the server reads it back with: generated code
//! passes the server's `static` (for `@http`, its whole `HttpSpec`) to both.
//!
//! Every answer that is not the bytes or the stream asked for is read the same
//! way: as `treat`'s envelope, `errors[]` first. An entry there is
//! [`ClientError::Api`] whatever the status line says — an RPC route answers a
//! failure with `200 OK` unless the service gave it another status, and an
//! `@http` route with the status the failure carries. Only a body that is not a
//! usable envelope falls back to the status: [`ClientError::Status`] for a
//! non-2xx one, such as a proxy's error page, and [`ClientError::Decode`] for a
//! 2xx one. A non-2xx answer, or one declared `x-rpc-status: error`, is read
//! only up to [`RpcClient::REFUSAL_LIMIT`]; a success is read whole.
//!
//! The types a generated client's callers name — [`ClientError`],
//! [`ApiErrors`], [`ClientResult`], [`ApiResponse`], [`ErrorMessage`],
//! [`ErrorSource`] and, with `stream`, `EventStream` and `StreamError` — live
//! here, so the generated module re-exports them and its callers keep the paths
//! they had when the generator emitted the types itself.
//!
//! No helper here needs the registry type: the code set of a failure is the
//! `C` of [`ClientResult<T, C>`], which each generated method fixes through its
//! return type.

mod call;
mod errors;
mod http;
mod query;
#[cfg(feature = "stream")]
mod stream;

pub use errors::{ApiErrors, ClientError, ClientResult};
/// The HTTP client a generated client is built over, re-exported so a caller
/// configuring one with [`RpcClient::with_http`] names the same `reqwest` the
/// runtime was built against.
pub use reqwest;
use serde::Serialize;
#[cfg(feature = "stream")]
pub use stream::{EventStream, StreamError};
/// The error entries are `treat`'s own, so the client decodes exactly the type
/// the server encodes and the two cannot drift apart.
pub use treat_core::{ErrorMessage, ErrorSource};

/// The response envelope, used for both success and failure.
///
/// Envelope-level `meta` is any JSON value, so `meta["page"]` and
/// `meta.get("page")` need no cast. A `serde_json::Value` rather than a
/// `serde_json::Map`: with `treat`'s `openapi` feature `meta` must have a
/// schema, which `utoipa` gives a `Value` and not a `Map`.
pub type ApiResponse<T> = treat_core::ApiResponse<T, serde_json::Value>;

/// The input of an operation that takes none: serializes as `{}`.
///
/// What a generated method without an input passes to [`RpcClient`], so the
/// server reads an empty object — the body it expects — rather than `null`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NoInput;

impl Serialize for NoInput {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        serializer.serialize_map(Some(0))?.end()
    }
}

/// The untyped client a generated one wraps: a base URL and a
/// [`reqwest::Client`].
///
/// Each method sends one kind of operation and reads its answer into the
/// `ClientResult<T, C>` the generated method returns; see the
/// [module documentation](self) for which method serves which operation.
///
/// Cloning is cheap and shares the underlying connection pool, so a clone per
/// task is the intended usage.
#[derive(Debug, Clone)]
pub struct RpcClient {
    base_url: String,
    http: reqwest::Client,
}

impl RpcClient {
    /// Client for the API served at `base_url`, which is an origin and
    /// optionally a prefix: operation paths from the SDL are absolute.
    ///
    /// The `reqwest::Client` is built with `reqwest`'s defaults as this build
    /// enables them, and `treat` builds `reqwest` without its default features
    /// — TLS among them. Without a TLS feature an `https` base URL fails when a
    /// request is sent, as [`ClientError::Transport`]: turn on `treat`'s
    /// `rpc-client-rustls` (`reqwest`'s `rustls`), or depend on `reqwest` with
    /// the TLS feature you want, or pass a configured client to
    /// [`with_http`](Self::with_http).
    ///
    /// # Panics
    ///
    /// When `reqwest` cannot build its default client, as
    /// [`reqwest::Client::new`] panics: a TLS backend that fails to
    /// initialize, or a system configuration the resolver cannot load. A
    /// caller that must not panic builds the client itself and handles the
    /// error: `RpcClient::with_http(base_url, reqwest::Client::builder().build()?)`.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_http(base_url, reqwest::Client::new())
    }

    /// Client using a caller-configured `reqwest::Client`, for timeouts,
    /// proxies, TLS roots and default headers.
    pub fn with_http(base_url: impl Into<String>, http: reqwest::Client) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http,
        }
    }

    /// The base URL, without a trailing `/`.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The underlying HTTP client, for callers that need to reuse the pool.
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// The URL of `path`, which may carry a query string already.
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
}
