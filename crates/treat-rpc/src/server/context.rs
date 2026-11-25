//! The caller of a session-guarded operation.

use super::Refusal;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

/// The caller, extracted before a session-guarded operation runs.
///
/// A newtype over the project's own context type so the extraction is one
/// [`ResolveContext`] impl the project writes once, rather than a bound a
/// generator would have to name. `FromRequestParts` and not `FromRequest`
/// because the body belongs to the operation's input: an extractor that
/// consumed it would have to be last, and the input's already is.
///
/// An RPC handler takes `Ctx(ctx): Ctx<S::Ctx>`, so axum answers a refusal
/// as it is; an `@http` one takes `Result<Ctx<S::Ctx>, Refusal>` and hands it to
/// [`Runtime::http_session`](crate::Runtime::http_session), which gives the
/// refusal a status.
#[derive(Debug, Clone)]
pub struct Ctx<T>(pub T);

// Not `#[async_trait]`: axum 0.8 declares `FromRequestParts` with native async,
// so a desugared impl would not match the trait's signature.
impl<T, S> FromRequestParts<S> for Ctx<T>
where
    T: ResolveContext,
    S: Send + Sync,
{
    type Rejection = Refusal;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        T::from_parts(parts).await.map(Ctx)
    }
}

/// How a request context is resolved from a request: the part of the context
/// contract that names nothing from a schema.
///
/// A generated server extends it with the access checks its schema calls for —
/// `trait FromRequestContext: ResolveContext { fn require_role(&self, role:
/// Role) -> Result<(), Refusal>; }` — since those name the schema's own role
/// and permission types. The project's context type implements both. Resolving
/// a session — the cookie, the exchange with whoever issues it, the user
/// lookup — is not describable in a schema, so it stays on the project's side.
///
/// Native async rather than `#[async_trait]`: `FromRequestParts` requires the
/// future to be `Send`, and a boxed one does not carry that bound where the
/// impl above needs it.
pub trait ResolveContext: Sized + Send + Sync + 'static {
    /// Resolve the caller from the request, or reject it.
    ///
    /// A rejection is a whole response ([`Refusal`], built from a `Response`
    /// or an `ApiError`) rather than a code, because failing to authenticate is
    /// not one of the operation's declared outcomes: it happens before the
    /// operation is reached. Answering with the envelope and a code the
    /// registry lists for it is the conventional shape.
    ///
    /// An `@http` handler answers a rejection left at `200` with `401`, since
    /// its callers read the status line. A rejection that is not about the
    /// caller — the session store unreachable — must carry its own status
    /// (`503`, `500`), which is kept, or such a caller reads it as a sign-out.
    ///
    /// "Left at `200`" is literal. Under `treat`'s `error-status-500` feature
    /// a refusal built from an `ApiError` without a status of its own already
    /// carries `500`, which is then kept like any other: an `@http` caller
    /// gets `500` rather than `401` (and rather than an access check's `403`).
    /// A project on that feature gives such a refusal its status itself —
    /// `ApiError::from(ErrorCode::Unauthenticated).with_status(401)`.
    fn from_parts(parts: &mut Parts) -> impl Future<Output = Result<Self, Refusal>> + Send;
}
