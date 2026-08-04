use crate::*;
use std::borrow::Cow;
use std::fmt::{Debug, Display};
use std::sync::Arc;

pub type Location = std::panic::Location<'static>;

pub trait ApiErrorCode: Debug + Clone + Display + Send + Sync {}
impl<T: Debug + Clone + Display + Send + Sync> ApiErrorCode for T {}

/// Default HTTP status carried by an [`ApiError`] when none is set explicitly.
///
/// By design a failure travels in the `errors[]` envelope, historically always
/// with `200 OK`; that is the default.
///
/// The `error-status-500` feature flips this to `500 Internal Server Error` for
/// callers who want an unhandled failure to look like one at the transport layer
/// — so clients that only check the status line, and monitoring that alerts on
/// `5xx`, both see the error. Setting a status explicitly with
/// [`ApiError::with_status`] or [`ApiError::with_code_status`] is unaffected
/// either way; only the unset case moves.
///
/// The flag is a Cargo feature, so it is crate-global and additive: enabling it
/// anywhere in the dependency graph changes the default for every dependant.
pub const DEFAULT_ERROR_STATUS: u16 = if cfg!(feature = "error-status-500") { 500 } else { 200 };

/// Status substituted when the configured one is not a valid HTTP status code.
///
/// Deliberately **not** `5xx`, and deliberately *not* [`DEFAULT_ERROR_STATUS`] —
/// this is the release-build safety net for a *bug in the calling code*, not a
/// statement about how failures should be reported, so `error-status-500` does
/// not move it. HTTP is the transport here: it delivered the response, and the
/// refusal itself lives in `errors[]`. A `500` would be indistinguishable from a
/// proxy/load-balancer failure — which can replace the body and lose the
/// envelope entirely — and would trip retry policies and alerting for what is a
/// valid, fully-delivered answer.
const INVALID_STATUS_FALLBACK: u16 = 200;

/// The range of status codes HTTP actually defines (RFC 9110 §15): the classes
/// run `1xx`..`5xx`, so `599` is the highest meaningful value.
///
/// Deliberately tighter than what [`http::StatusCode`] accepts (`100..=999`).
/// That type only rejects values it cannot *represent*; a `6xx`–`9xx` code is
/// representable but belongs to no class, so it reaches the client as
/// `<unknown status code>` and leaves proxies and clients to guess. Treating it
/// as the caller-side bug it is keeps the transport meaningful.
///
/// [`http::StatusCode`]: https://docs.rs/http/latest/http/status/struct.StatusCode.html
pub const VALID_STATUS_RANGE: std::ops::RangeInclusive<u16> = 100..=599;

/// Resolve a configured status into a real HTTP status code, used by every
/// framework adapter so they agree.
///
/// A value outside [`VALID_STATUS_RANGE`] means the *calling code* is buggy —
/// [`ApiError::with_status`] or an [`ApiErrorStatus`] mapping produced a bogus
/// number — not that the operation failed differently. Both setters
/// `debug_assert!` on it, so the bug surfaces in tests; in release the status
/// falls back to `200`.
pub fn resolve_status(status: u16) -> u16 {
    match VALID_STATUS_RANGE.contains(&status) {
        true => status,
        false => INVALID_STATUS_FALLBACK,
    }
}

/// Opt-in mapping from an error code to an HTTP status.
///
/// The adapters read the per-error status field (see [`ApiError::status`] /
/// [`ApiError::with_status`]); this trait lets a code enum supply that value so
/// callers don't repeat it. Implement it on your code type and seed the field
/// with [`ApiError::with_code_status`]:
///
/// ```
/// use treat_core::{error, ApiErrorStatus};
/// #[derive(Debug, Clone)]
/// enum Code { NotFound }
/// impl std::fmt::Display for Code {
///     fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { write!(f, "not_found") }
/// }
/// impl ApiErrorStatus for Code {
///     fn status_code(&self) -> u16 { match self { Code::NotFound => 404 } }
/// }
/// let err = error(Code::NotFound).with_code_status();
/// assert_eq!(err.status(), 404);
/// ```
pub trait ApiErrorStatus {
    /// The HTTP status this code should map to.
    fn status_code(&self) -> u16;
}

/// A typed API error: a `code` (`C`, `&'static str` by default), an optional
/// human `message`, arbitrary `meta`, an optional source chain ([`erris::Report`])
/// and the `#[track_caller]` location where it was raised.
///
/// Create one with [`error()`] and enrich it with the `with_*` builders; convert
/// foreign errors with the [`OkOrError`] / [`WrapApiError`] extensions. See the
/// crate-level docs for the full guide.
///
/// ```
/// let err = treat_core::error("not_found").with_message("no such user");
/// assert_eq!(*err.code(), "not_found");
/// ```
#[derive(Clone)]
pub struct ApiError<C: ApiErrorCode + 'static = &'static str> {
    // Inner Box is used to minimize the size of the Result type and reduce stack memory usage.
    pub(crate) boxed: Box<ApiErrorInner<C>>,
}

#[derive(Debug, Clone)]
pub struct ApiErrorInner<C: ApiErrorCode> {
    // Fields are crate-private: mutate through the `with_*` builders and read
    // through the accessors (`code()`, `message()`, ...). This keeps `location`
    // (captured via `#[track_caller]`) and the source chain tamper-proof.
    pub(crate) code: C,
    pub(crate) message: Option<Cow<'static, str>>,
    pub(crate) meta: Option<serde_json::Value>,
    pub(crate) error_source: Option<ErrorSource>,
    pub(crate) type_uri: Option<Cow<'static, str>>,
    pub(crate) instance: Option<String>,
    /// Transport-level HTTP status. `None` → [`DEFAULT_ERROR_STATUS`]. Not part
    /// of the serialized body — the adapters read it for the response status.
    pub(crate) status: Option<u16>,
    pub(crate) source: Option<Arc<erris::Report>>,
    pub(crate) verbose: bool,
    pub(crate) location: &'static Location,
    #[cfg(feature = "spantrace")]
    pub(crate) spantrace: erris::SpanTrace,
}

impl<C: ApiErrorCode> ApiError<C> {
    #[inline]
    pub fn err<T: ResponseData>(self) -> Result<ApiResponse<T, NoMeta>, ApiError<C>> {
        Err(self)
    }

    #[inline]
    pub fn into_result<T: ResponseData>(self) -> Result<ApiResponse<T, NoMeta>, ApiError<C>> {
        Err(self)
    }

    pub fn is_verbose(&self) -> bool {
        // NOTE enforce verbose mode
        #[cfg(feature = "verbose-error")]
        return true;

        #[cfg(not(feature = "verbose-error"))]
        return self.verbose;
    }

    pub fn code(&self) -> &C {
        &self.code
    }

    pub fn message(&self) -> Option<&Cow<'static, str>> {
        self.message.as_ref()
    }

    pub fn source(&self) -> Option<&erris::Report> {
        self.source.as_ref().map(|v| v.as_ref())
    }

    pub fn meta(&self) -> Option<&serde_json::Value> {
        self.meta.as_ref()
    }

    /// The error locator (JSON:API `source`), if set via
    /// [`with_pointer`](Self::with_pointer) / [`with_parameter`](Self::with_parameter)
    /// / [`with_header`](Self::with_header).
    pub fn error_source(&self) -> Option<&ErrorSource> {
        self.error_source.as_ref()
    }

    /// The RFC 9457 `type` URI, if set via [`with_type`](Self::with_type).
    pub fn type_uri(&self) -> Option<&str> {
        self.type_uri.as_deref()
    }

    /// The RFC 9457 `instance`, if set via [`with_instance`](Self::with_instance).
    pub fn instance(&self) -> Option<&str> {
        self.instance.as_deref()
    }

    /// The transport HTTP status. Returns [`DEFAULT_ERROR_STATUS`] (`200`, or
    /// `500` under the `error-status-500` feature) unless set via
    /// [`with_status`](Self::with_status) or [`with_code_status`](Self::with_code_status).
    ///
    /// Wrapping does **not** inherit: `error("outer").with_error(inner_404)`
    /// reports the default, because the wrapping layer decides what the client
    /// sees.
    /// Use [`with_source_status`](Self::with_source_status) to carry a nested
    /// error's status outward.
    pub fn status(&self) -> u16 {
        self.status.unwrap_or(DEFAULT_ERROR_STATUS)
    }

    /// Whether a transport status was set explicitly (as opposed to falling back
    /// to [`DEFAULT_ERROR_STATUS`]).
    pub fn has_status(&self) -> bool {
        self.status.is_some()
    }

    /// Adopt `status` unless one was already set explicitly on `self`.
    ///
    /// Wrapping deliberately drops the inner error's status (see
    /// [`status`](Self::status)); this opts back in for the common case of
    /// re-raising a nested failure while keeping its HTTP semantics:
    ///
    /// ```
    /// use treat_core::error;
    /// let inner = error("not_found").with_status(404);
    /// let inner_status = inner.status();
    /// let outer = error("lookup_failed").with_error(inner).with_source_status(inner_status);
    /// assert_eq!(outer.status(), 404);
    /// ```
    pub fn with_source_status(mut self, status: u16) -> Self {
        if self.boxed.status.is_none() {
            self.boxed.status = Some(status);
        }
        self
    }

    pub fn to_error_message(&self) -> ErrorMessage {
        ErrorMessage {
            code: self.code.to_string(),
            message: self.format_message(),
            type_uri: self.type_uri.as_ref().map(ToString::to_string),
            instance: self.instance.clone(),
            source: self.error_source.clone().filter(|s| !s.is_empty()),
            meta: self.meta.clone(),
        }
    }

    pub fn to_error_message_with(&self, verbose: bool) -> ErrorMessage {
        ErrorMessage {
            code: self.code.to_string(),
            // `verbose == false` must honour the argument: return the raw message,
            // not `format_message()` (which re-checks `is_verbose()` and would
            // re-expand the source under `.with_verbose()` or the `verbose-error`
            // feature). Mirrors the `false` arm of `into_error_message`.
            message: match verbose {
                true => self.format_message_verbose(),
                false => self.message.as_ref().map(ToString::to_string),
            },
            type_uri: self.type_uri.as_ref().map(ToString::to_string),
            instance: self.instance.clone(),
            source: self.error_source.clone().filter(|s| !s.is_empty()),
            meta: self.meta.clone(),
        }
    }

    pub fn into_error_message(self) -> ErrorMessage {
        let code = self.code.to_string();
        let message = match self.is_verbose() {
            true => self.format_message_verbose(),
            false => self.boxed.message.map(|v| v.to_string()),
        };
        ErrorMessage {
            code,
            message,
            type_uri: self.boxed.type_uri.map(|v| v.to_string()),
            instance: self.boxed.instance,
            source: self.boxed.error_source.filter(|s| !s.is_empty()),
            meta: self.boxed.meta,
        }
    }

    pub fn format_message_verbose(&self) -> Option<String> {
        match (&self.message, &self.source) {
            (Some(message), Some(source)) => Some(format!("{message}, {source}")),
            (Some(message), None) => Some(message.to_string()),
            (None, Some(source)) => Some(format!("{source}")),
            (None, None) => None,
        }
    }

    pub fn format_message(&self) -> Option<String> {
        if self.is_verbose() {
            return self.format_message_verbose();
        }

        self.message.as_ref().map(ToString::to_string)
    }

    pub fn with_message(mut self, message: impl Into<Cow<'static, str>>) -> Self {
        self.boxed.message = Some(message.into());
        self
    }

    pub fn with_meta(mut self, meta: impl Into<serde_json::Value>) -> Self {
        self.boxed.meta = Some(meta.into());
        self
    }

    /// Set the whole error locator (JSON:API `source`) at once.
    pub fn with_error_source(mut self, source: ErrorSource) -> Self {
        self.boxed.error_source = Some(source);
        self
    }

    /// Point at the offending value with a JSON Pointer (RFC 6901),
    /// e.g. `/data/attributes/email`.
    pub fn with_pointer(mut self, pointer: impl Into<String>) -> Self {
        self.boxed.error_source.get_or_insert_default().pointer = Some(pointer.into());
        self
    }

    /// Point at the offending query parameter by name.
    pub fn with_parameter(mut self, parameter: impl Into<String>) -> Self {
        self.boxed.error_source.get_or_insert_default().parameter = Some(parameter.into());
        self
    }

    /// Point at the offending request header by name.
    pub fn with_header(mut self, header: impl Into<String>) -> Self {
        self.boxed.error_source.get_or_insert_default().header = Some(header.into());
        self
    }

    /// Set the RFC 9457 `type` URI (a link to documentation for this error kind).
    pub fn with_type(mut self, type_uri: impl Into<Cow<'static, str>>) -> Self {
        self.boxed.type_uri = Some(type_uri.into());
        self
    }

    /// Set the RFC 9457 `instance` (id/URI of this specific occurrence, e.g. a request id).
    pub fn with_instance(mut self, instance: impl Into<String>) -> Self {
        self.boxed.instance = Some(instance.into());
        self
    }

    /// Set the transport HTTP status the adapters return (e.g. `404`). This is
    /// not serialized into the body; it only affects the response status line.
    /// Shares the status field with [`with_code_status`](Self::with_code_status),
    /// so whichever is called last wins.
    ///
    /// `status` must be a valid HTTP status code ([`VALID_STATUS_RANGE`]). A
    /// debug build panics otherwise; a release build falls back to `200` at the
    /// adapter (see [`resolve_status`]).
    #[track_caller]
    pub fn with_status(mut self, status: u16) -> Self {
        debug_assert!(
            VALID_STATUS_RANGE.contains(&status),
            "invalid HTTP status {status} passed to `with_status`; must be {}..={}",
            VALID_STATUS_RANGE.start(),
            VALID_STATUS_RANGE.end(),
        );
        self.boxed.status = Some(status);
        self
    }

    pub fn with_verbose(mut self) -> Self {
        self.boxed.verbose = true;
        self
    }

    #[inline]
    #[track_caller]
    pub fn with_error<T: Into<erris::Report>>(self, err: T) -> Self {
        self.with_source(err.into())
    }

    #[inline]
    #[track_caller]
    pub fn with_source(mut self, source: erris::Report) -> Self {
        match self.boxed.source.take() {
            Some(existed_source) => {
                self.boxed.source = Some(Arc::new(
                    erris::Report::from_arc_report(existed_source).with_err(source),
                ));
            }
            None => {
                self.boxed.source = Some(Arc::new(source));
            }
        }
        self
    }

    #[inline]
    #[track_caller]
    pub fn track(self) -> Self {
        self.with_source(erris::Report::new_transparent())
    }
}

impl<C: ApiErrorCode + ApiErrorStatus> ApiError<C> {
    /// Seed the transport status from the code's [`ApiErrorStatus`] mapping.
    /// Shares the status field with [`with_status`](Self::with_status), so
    /// whichever is called last wins.
    ///
    /// The mapping must yield a valid HTTP status code ([`VALID_STATUS_RANGE`]);
    /// see [`with_status`](Self::with_status) for the out-of-range behaviour.
    #[track_caller]
    pub fn with_code_status(mut self) -> Self {
        let status = self.boxed.code.status_code();
        debug_assert!(
            VALID_STATUS_RANGE.contains(&status),
            "`ApiErrorStatus::status_code` returned invalid HTTP status {status}; must be {}..={}",
            VALID_STATUS_RANGE.start(),
            VALID_STATUS_RANGE.end(),
        );
        self.boxed.status = Some(status);
        self
    }
}

impl<C: ApiErrorCode> std::ops::Deref for ApiError<C> {
    type Target = ApiErrorInner<C>;

    fn deref(&self) -> &Self::Target {
        self.boxed.as_ref()
    }
}

impl<C: ApiErrorCode> std::fmt::Display for ApiError<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "treat error: {}", self.code)?;
        if let Some(message) = self.format_message()
            && !message.is_empty()
        {
            write!(f, ", message: {message}")?;
        }
        Ok(())
    }
}

impl<C: ApiErrorCode> std::error::Error for ApiError<C> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|v| v.as_ref().as_ref())
    }

    #[cfg(feature = "nightly-provide")]
    fn provide<'a>(&'a self, demand: &mut std::error::Request<'a>) {
        demand.provide_value(self.location);

        if let Some(report) = &self.source {
            demand.provide_ref::<erris::Report>(report);

            #[cfg(feature = "backtrace")]
            if let Some(backtrace) = report.backtrace() {
                demand.provide_ref::<erris::Backtrace>(backtrace);
            }

            #[cfg(feature = "spantrace")]
            demand.provide_ref::<erris::SpanTrace>(report.spantrace());
        }
    }
}

/// Create a bare [`ApiError`] from a code. Captures the caller location.
///
/// ```
/// let err = treat_core::error("rate_limited");
/// assert_eq!(*err.code(), "rate_limited");
/// ```
#[track_caller]
pub fn error<C: ApiErrorCode>(code: C) -> ApiError<C> {
    ApiError {
        boxed: Box::new(ApiErrorInner {
            code,
            message: None,
            meta: None,
            error_source: None,
            type_uri: None,
            instance: None,
            status: None,
            source: None,
            verbose: false,
            location: std::panic::Location::caller(),
            #[cfg(feature = "spantrace")]
            spantrace: erris::SpanTrace::capture(),
        }),
    }
}

#[track_caller]
pub fn error_and_message<C: ApiErrorCode>(code: C, message: impl Into<Cow<'static, str>>) -> ApiError<C> {
    ApiError {
        boxed: Box::new(ApiErrorInner {
            code,
            message: Some(message.into()),
            meta: None,
            error_source: None,
            type_uri: None,
            instance: None,
            status: None,
            source: None,
            verbose: false,
            location: std::panic::Location::caller(),
            #[cfg(feature = "spantrace")]
            spantrace: erris::SpanTrace::capture(),
        }),
    }
}

#[track_caller]
pub fn wrap_error<C: ApiErrorCode>(err: erris::Report, code: C, message: impl Into<Cow<'static, str>>) -> ApiError<C> {
    ApiError {
        boxed: Box::new(ApiErrorInner {
            code,
            message: Some(message.into()),
            meta: None,
            error_source: None,
            type_uri: None,
            instance: None,
            status: None,
            source: Some(Arc::new(err)),
            verbose: false,
            location: std::panic::Location::caller(),
            #[cfg(feature = "spantrace")]
            spantrace: erris::SpanTrace::capture(),
        }),
    }
}

/// Object-safe view of an [`ApiError<C>`] with the concrete code type erased,
/// used to stash the error in framework response extensions (see
/// [`crate::response_get_api_error`]). Only `&self`
/// methods are exposed — an owning `into_error_message` is unreachable through
/// `dyn`/`Arc`, so it lives on [`ApiError`] itself.
pub trait ApiErrorHandler: Debug + Send + Sync {
    fn is_verbose(&self) -> bool;
    fn code(&self) -> String;
    fn message(&self) -> Option<&Cow<'static, str>>;
    fn source(&self) -> Option<&erris::Report>;
    fn meta(&self) -> Option<&serde_json::Value>;
    fn to_error_message(&self) -> ErrorMessage;
    fn format_message(&self) -> Option<String>;
    fn format_message_verbose(&self) -> Option<String>;

    /// The transport HTTP status, as [`ApiError::status`] reports it —
    /// [`DEFAULT_ERROR_STATUS`] when none was set explicitly.
    ///
    /// This is the *configured* value, not necessarily the one on the wire: the
    /// adapters run it through [`resolve_status`] first. Pair the two to log
    /// what the client actually saw.
    fn status(&self) -> u16;

    /// Whether a transport status was set explicitly, as
    /// [`ApiError::has_status`] reports it. Lets an observer tell a deliberate
    /// `200` from the unset default.
    fn has_status(&self) -> bool;

    /// The error locator (JSON:API `source`), as [`ApiError::error_source`]
    /// reports it.
    fn error_source(&self) -> Option<&ErrorSource>;

    /// The `#[track_caller]` location where the error was raised, as captured by
    /// [`error()`]. The single most useful field for tracing a failure back to
    /// its origin, and unreachable once the code type is erased.
    fn location(&self) -> &'static Location;
}

impl<C: ApiErrorCode> ApiErrorHandler for ApiError<C> {
    fn is_verbose(&self) -> bool {
        ApiError::<C>::is_verbose(self)
    }

    fn code(&self) -> String {
        self.code.to_string()
    }

    fn message(&self) -> Option<&Cow<'static, str>> {
        ApiError::<C>::message(self)
    }

    fn source(&self) -> Option<&erris::Report> {
        ApiError::<C>::source(self)
    }

    fn meta(&self) -> Option<&serde_json::Value> {
        ApiError::<C>::meta(self)
    }

    fn to_error_message(&self) -> ErrorMessage {
        ApiError::<C>::to_error_message(self)
    }

    fn format_message(&self) -> Option<String> {
        ApiError::<C>::format_message(self)
    }

    fn format_message_verbose(&self) -> Option<String> {
        ApiError::<C>::format_message_verbose(self)
    }

    fn status(&self) -> u16 {
        ApiError::<C>::status(self)
    }

    fn has_status(&self) -> bool {
        ApiError::<C>::has_status(self)
    }

    fn error_source(&self) -> Option<&ErrorSource> {
        ApiError::<C>::error_source(self)
    }

    fn location(&self) -> &'static Location {
        self.location
    }
}

impl<C: ApiErrorCode> std::fmt::Debug for ApiError<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if f.alternate() {
            return f
                .debug_struct("ApiError")
                .field("code", &self.code.to_string())
                .field("message", &self.message)
                .field("error_source", &self.error_source)
                .field("type_uri", &self.type_uri)
                .field("instance", &self.instance)
                .field("status", &self.status)
                .field("source", &self.source)
                .field("verbose", &self.verbose)
                .field("location", &self.location)
                .field(
                    "spantrace",
                    #[cfg(feature = "spantrace")]
                    &self.spantrace,
                    #[cfg(not(feature = "spantrace"))]
                    &Option::<erris::SpanTrace>::None,
                )
                .finish();
        }

        // `erris::debug_error_chain` renders a `ReportError`, and building one
        // needs an owned error — hence the clone. It copies the `ApiErrorInner`
        // box and bumps the source `Arc`'s refcount; the cause chain itself is
        // shared, not deep-copied. Use `{:#?}` for an allocation-free field dump.
        let report = erris::Report::from_error(self.clone());
        erris::debug_error_chain(&report, f)
    }
}
