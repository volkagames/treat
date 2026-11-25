//! The axum server runtime (feature `server`, `treat`'s `rpc-server`).
//!
//! Everything a generated handler calls is an associated function of
//! [`Runtime<R>`](crate::Runtime), so a generated server aliases it once —
//! `type Rt = treat::rpc::Runtime<ErrorCode>;` — and a handler is a few lines:
//!
//! ```ignore
//! async fn users_rename<S: UsersService>(
//!     State(service): State<S>,
//!     request: axum::extract::Request,
//! ) -> Result<Response, Refusal> {
//!     let input: RenameInput = Rt::json_input(request).await?;
//!     Ok(Rt::respond(service.rename(input).await))
//! }
//! ```
//!
//! A handler returns `Result<Response, Refusal>` so each step that can refuse
//! the request is one `?`: a [`Refusal`] is already the whole response, built
//! the way the wire protocol says that step refuses.
//!
//! The building blocks those helpers are made of, which need no registry, are
//! plain items of this module:
//!
//! - reading an input from the URL — [`QueryField`], [`QueryKind`] and the
//!   `Runtime` helpers `input_from_query`, `url_pairs`, `url_text`,
//!   `input_from_pairs`, `query_value`, `invalid_query`;
//! - `@http` operations — [`http_status`], [`override_pairs`] and the `Runtime`
//!   helpers `http_input`, `read_http_input`, `respond_http`, `http_session`,
//!   `http_access`, each reading the operation's [`HttpSpec`](crate::HttpSpec);
//! - request context — [`Ctx`] and [`ResolveContext`];
//! - [`Refusal`], what every step fails with;
//! - with feature `stream` — `EventStream`, `event_stream`, `RawBody`,
//!   `RawRequest`, `sse_response`, `RAW_BODY_LIMIT`, `RAW_BODY_IGNORED_HEADERS`
//!   and the `Runtime` helpers `raw_body`, `raw_response`, `respond_raw`,
//!   `respond_events`.
//!
//! Every refusal is the `treat` envelope with a registry code: a request the
//! runtime cannot read is [`RuntimeCodes::INVALID_BODY`](crate::RuntimeCodes::INVALID_BODY),
//! a failure of its own [`RuntimeCodes::INTERNAL_ERROR`](crate::RuntimeCodes::INTERNAL_ERROR).
//! An RPC body is read only under a JSON `Content-Type`, and a raw one only
//! under a type its operation declares (`415` otherwise, see
//! `Runtime::no_input` for why); one too large answers `413`, and a JSON one
//! that does not parse `422` with a pointer to where it went wrong. The other
//! refusals of an RPC route answer `200`, since its callers read `errors[]`; an
//! `@http` route gives them the status its callers read (see [`http_status`]).

mod context;
mod handler;
mod http;
mod query;
mod refusal;
#[cfg(feature = "stream")]
mod stream;

pub use context::*;
pub use http::*;
pub use query::*;
pub use refusal::*;
#[cfg(feature = "stream")]
pub use stream::*;
