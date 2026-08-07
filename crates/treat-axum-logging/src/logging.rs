use http::Request;
use std::boxed::Box;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::{Layer, Service};
use tracing::Instrument;
use treat_core::ApiErrorHandler;

/// The logger signature: called once per response that carries a `treat` error.
type ErrorLogger = fn(&dyn ApiErrorHandler);

/// [`tower::Layer`] that logs the [`ApiErrorHandler`] a response carries.
///
/// ```
/// # use treat_axum_logging::LoggingLayer;
/// let layer = LoggingLayer::new();
/// ```
#[derive(Clone, Copy, Debug)]
pub struct LoggingLayer<L = ErrorLogger> {
    error_handler: L,
}

impl LoggingLayer {
    pub fn new() -> Self {
        Self {
            error_handler: error_log as ErrorLogger,
        }
    }
}

impl<L> LoggingLayer<L>
where
    L: Fn(&dyn ApiErrorHandler) + Clone,
{
    /// Swap the default logger for `error_handler` — e.g. [`info_log`] or
    /// [`debug_log`], or a closure of your own.
    pub fn with_error_logger(error_handler: L) -> Self {
        Self { error_handler }
    }
}

impl Default for LoggingLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S, L> Layer<S> for LoggingLayer<L>
where
    L: Fn(&dyn ApiErrorHandler) + Clone,
{
    type Service = LoggingMiddleware<S, L>;

    fn layer(&self, inner: S) -> Self::Service {
        LoggingMiddleware {
            state: inner,
            error_handler: self.error_handler.clone(),
        }
    }
}

/// The [`Service`] produced by [`LoggingLayer`].
#[derive(Clone, Debug)]
pub struct LoggingMiddleware<S, L = ErrorLogger> {
    state: S,
    error_handler: L,
}

impl<ReqBody, ResBody, S, L> Service<Request<ReqBody>> for LoggingMiddleware<S, L>
where
    S: Service<Request<ReqBody>, Response = http::Response<ResBody>> + Clone + Send + 'static,
    S::Error: std::fmt::Debug,
    S::Future: Send + 'static,
    L: Fn(&dyn ApiErrorHandler) + Clone + Send + 'static,
    ReqBody: Send + 'static,
    ResBody: http_body::Body,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.state.poll_ready(cx)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        // Opened before the inner service runs, so everything it logs — and the
        // error logged below — is recorded under this span.
        let span = crate::root_span_on_request_start(&request);

        // `poll_ready` reserves capacity on *this* instance, so the future has to
        // drive the instance it was called on: cloning would move an unreserved
        // copy in and leave the reservation stranded here, which a service is
        // permitted to panic over. Swapping takes the ready one and leaves the
        // fresh clone behind for the next request.
        //
        // Verbatim from `tower::Service`'s own docs, "Be careful when cloning
        // inner services" — see `tower_service::Service` (tower-service 0.3).
        let clone = self.state.clone();
        // take the service that was ready
        let mut state = std::mem::replace(&mut self.state, clone);
        let error_handler = self.error_handler.clone();

        let request_span = span.clone();
        let future = async move {
            match state.call(request).await {
                Ok(response) => {
                    let response = crate::root_span_on_response_end(&request_span, response);

                    if let Some(api_error) = treat_core::response_get_api_error(&response) {
                        error_handler(api_error.as_ref());
                    }
                    Ok(response)
                }
                Err(err) => {
                    // Not an `ApiError`: the inner service failed before one could
                    // be built, so `error_handler` has nothing to inspect.
                    tracing::error!(error = ?err, "response error");
                    Err(err)
                }
            }
        };

        Box::pin(future.instrument(span))
    }
}

/// The default logger: severity follows the error, not the call site.
///
/// An error carrying a source chain means something failed underneath the
/// service rather than the caller asking for something absent, so it is worth an
/// `error!`; a plain business error (`user_not_found` and friends) is routine and
/// logs at `debug!`. This is the last point that still holds the `erris` report —
/// past it only the code and message reach the client.
pub fn error_log(err: &dyn ApiErrorHandler) {
    match err.source() {
        Some(report) => {
            tracing::error!(
                error_code = %err.code(),
                error_location = %err.location(),
                cause = ?report,
                "{}",
                err.format_message_verbose().unwrap_or_default()
            )
        }
        None => {
            tracing::debug!(
                error_code = %err.code(),
                error_location = %err.location(),
                "{}",
                err.format_message().unwrap_or_default()
            )
        }
    }
}

/// Logs every error at `info!`, whatever its severity.
pub fn info_log(err: &dyn ApiErrorHandler) {
    tracing::info!(
        error_code = %err.code(),
        error_location = %err.location(),
        cause = ?err.source(),
        "{}",
        err.format_message().unwrap_or_default()
    );
}

/// Logs every error at `debug!`, whatever its severity.
pub fn debug_log(err: &dyn ApiErrorHandler) {
    tracing::debug!(
        error_code = %err.code(),
        error_location = %err.location(),
        cause = ?err.source(),
        "{}",
        err.format_message().unwrap_or_default()
    );
}
