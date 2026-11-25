use super::{DefaultRootSpanBuilder, RequestId, RootSpan, RootSpanBuilder};
use actix_web::body::{BodySize, MessageBody};
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::web::Bytes;
use actix_web::{Error, HttpMessage};
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::task::{Context, Poll};
use tracing::Span;
use treat_core::ApiErrorHandler;

/// The actix-web tracing middleware.
///
/// Per request it resolves a [`RequestId`] (adopting a UUID `x-request-id`),
/// opens the root span through `R` (a [`RootSpanBuilder`]), calls the error
/// logger `L` for a failed request (or logs `request success` at `debug!`),
/// and writes the id back on the response as `x-request-id`.
///
/// An inner service that resolves to `Err` (a middleware wrapped inside the
/// logger rejecting the request) is logged and passed on untouched: it has no
/// response yet, so it gets no `x-request-id`. A middleware that should keep
/// the id answers `Ok(req.error_response(err))` instead of `Err(err)`.
///
/// ```ignore
/// // default builder, default logger (`error_log`)
/// App::new().wrap(Logger::default());
/// // default builder, typed-code logger
/// App::new().wrap(Logger::with_error_logger(error_log_for::<MyCode>));
/// // custom builder, typed-code logger
/// App::new().wrap(Logger::<MyRootSpan>::new().error_logger(error_log_for::<MyCode>));
/// ```
pub struct Logger<R: RootSpanBuilder = DefaultRootSpanBuilder, L: Fn(&actix_web::Error) + Clone = fn(&actix_web::Error)>
{
    error_handler: L,
    root_span_builder: std::marker::PhantomData<R>,
}

impl<R: RootSpanBuilder, L: Fn(&actix_web::Error) + Clone> Clone for Logger<R, L> {
    fn clone(&self) -> Self {
        Self {
            error_handler: self.error_handler.clone(),
            root_span_builder: std::marker::PhantomData,
        }
    }
}

impl<L: Fn(&actix_web::Error) + Clone> Logger<DefaultRootSpanBuilder, L> {
    /// The default root span builder with `error_handler` as the error logger.
    /// For a custom [`RootSpanBuilder`], use [`Logger::error_logger`].
    pub fn with_error_logger(error_handler: L) -> Self {
        Self {
            root_span_builder: Default::default(),
            error_handler,
        }
    }
}

impl Default for Logger<DefaultRootSpanBuilder, fn(&actix_web::Error)> {
    fn default() -> Self {
        Self {
            root_span_builder: Default::default(),
            error_handler: error_log,
        }
    }
}

impl<R: RootSpanBuilder> Logger<R, fn(&actix_web::Error)> {
    /// A logger with the root span builder `R` and the default [`error_log`]:
    /// `Logger::<MyRootSpan>::new()`.
    pub fn new() -> Logger<R, fn(&actix_web::Error)> {
        Logger {
            root_span_builder: Default::default(),
            error_handler: error_log,
        }
    }
}

impl<R: RootSpanBuilder, L: Fn(&actix_web::Error) + Clone> Logger<R, L> {
    /// Replace the error logger, keeping the root span builder `R`:
    /// `Logger::<MyRootSpan>::new().error_logger(error_log_for::<MyCode>)`.
    pub fn error_logger<L2: Fn(&actix_web::Error) + Clone>(self, error_handler: L2) -> Logger<R, L2> {
        Logger {
            root_span_builder: Default::default(),
            error_handler,
        }
    }
}

impl<S, B, RootSpan, L> Transform<S, ServiceRequest> for Logger<RootSpan, L>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: MessageBody + 'static,
    RootSpan: RootSpanBuilder,
    L: Fn(&actix_web::Error) + Clone,
{
    type Response = ServiceResponse<StreamSpan<B>>;
    type Error = Error;
    type Transform = LoggerTransform<S, RootSpan, L>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(LoggerTransform {
            service,
            error_handler: self.error_handler.clone(),
            root_span_builder: std::marker::PhantomData,
        }))
    }
}

#[doc(hidden)]
pub struct LoggerTransform<S, RootSpanBuilder, L: Fn(&actix_web::Error)> {
    service: S,
    error_handler: L,
    root_span_builder: std::marker::PhantomData<RootSpanBuilder>,
}

#[allow(clippy::type_complexity)]
impl<S, B, RootSpanType, L> Service<ServiceRequest> for LoggerTransform<S, RootSpanType, L>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: MessageBody + 'static,
    RootSpanType: RootSpanBuilder,
    L: Fn(&actix_web::Error) + Clone,
{
    type Response = ServiceResponse<StreamSpan<B>>;
    type Error = Error;
    type Future = TracingResponse<S::Future, RootSpanType, L>;

    actix_web::dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        // Adopt the caller's id when present so one identifier spans the whole
        // call chain; otherwise start a new one here.
        let request_id = RequestId::from_headers(req.headers());
        req.extensions_mut().insert(request_id);
        let root_span = RootSpanType::on_request_start(&req);

        let root_span_wrapper = RootSpan::new(root_span.clone());
        req.extensions_mut().insert(root_span_wrapper);

        let fut = root_span.in_scope(|| self.service.call(req));

        TracingResponse {
            fut,
            error_handler: self.error_handler.clone(),
            span: root_span,
            request_id,
            _root_span_type: std::marker::PhantomData,
        }
    }
}

#[doc(hidden)]
#[pin_project::pin_project]
pub struct TracingResponse<F, RootSpanType, L: Fn(&actix_web::Error)> {
    #[pin]
    fut: F,
    error_handler: L,
    span: Span,
    request_id: RequestId,
    _root_span_type: std::marker::PhantomData<RootSpanType>,
}

#[doc(hidden)]
#[pin_project::pin_project]
pub struct StreamSpan<B> {
    #[pin]
    body: B,
    span: Span,
}

impl<F, B, RootSpanType, L> Future for TracingResponse<F, RootSpanType, L>
where
    F: Future<Output = Result<ServiceResponse<B>, Error>>,
    B: MessageBody + 'static,
    RootSpanType: RootSpanBuilder,
    L: Fn(&actix_web::Error) + Clone,
{
    type Output = Result<ServiceResponse<StreamSpan<B>>, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let fut = this.fut;
        let span = this.span;
        let error_handler = this.error_handler;
        let request_id = *this.request_id;

        span.in_scope(|| {
            let polled = fut.poll(cx);
            let Poll::Ready(mut outcome) = polled else {
                return Poll::Pending;
            };

            RootSpanType::on_request_end(span.clone(), &outcome);

            match &outcome {
                Ok(response) => {
                    match response.response().error() {
                        Some(err) => error_handler(err),
                        None => tracing::debug!("request success"),
                    }
                }
                Err(err) => error_handler(err),
            }

            // A handler that set the header itself has the final say; only fill
            // in the gap so the caller can always correlate the response with a
            // log. An `Err` has no response to put it on.
            if let Ok(response) = &mut outcome {
                let headers = response.headers_mut();
                if !headers.contains_key(&crate::X_REQUEST_ID) {
                    headers.insert(crate::X_REQUEST_ID, request_id.to_header_value());
                }
            }

            Poll::Ready(outcome.map(|service_response| {
                service_response.map_body(|_, body| {
                    StreamSpan {
                        body,
                        span: span.clone(),
                    }
                })
            }))
        })
    }
}

impl<B> MessageBody for StreamSpan<B>
where
    B: MessageBody,
{
    type Error = B::Error;

    fn size(&self) -> BodySize {
        self.body.size()
    }

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Bytes, Self::Error>>> {
        let this = self.project();

        let body = this.body;
        let span = this.span;
        span.in_scope(|| body.poll_next(cx))
    }
}

/// Log an `ApiError` whose code type is `C` with structured fields.
///
/// `as_error::<T>` is a `TypeId` check, so it can only name a *concrete*
/// `ApiError<C>`; there is no way to ask "any `ApiError`". `ResponseError` is
/// only `Debug + Display`, so it cannot be upcast to `dyn Error` to reach
/// [`ApiErrorHandler`](treat_core::ApiErrorHandler) either. Hence the explicit
/// type parameter: a service with a typed code enum picks the matching logger
/// with [`error_log_for`] / [`debug_log_for`], which mirrors how
/// `report_collect_messages` takes the caller's `C`.
///
/// Returns `false` when the error is not an `ApiError<C>`, so callers can chain
/// a fallback.
fn log_api_error<C: treat_core::ApiErrorCode + 'static>(err: &actix_web::Error, level: tracing::Level) -> bool {
    let Some(err) = err.as_error::<treat_core::ApiError<C>>() else {
        return false;
    };
    let message = err.format_message().unwrap_or_default();
    let location = ApiErrorHandler::location(err);
    let hops = err.format_hops();
    // `tracing` needs a const level per call site, so the arms are spelled out.
    match level {
        tracing::Level::ERROR => {
            tracing::error!(
                error_code = %err.code(),
                error_location = %location,
                error_hops = hops.as_deref(),
                error_source = ?err.source(),
                %message
            )
        }
        _ => {
            tracing::debug!(
                error_code = %err.code(),
                error_location = %location,
                error_hops = hops.as_deref(),
                error_source = ?err.source(),
                %message
            )
        }
    }
    true
}

/// [`error_log`] at `debug!`.
#[inline(always)]
pub fn debug_log(err: &actix_web::Error) {
    debug_log_for::<&'static str>(err)
}

/// The [`Logger`](crate::Logger)'s default error logger, at `error!`.
///
/// Logs `error_code`, `error_location`, `error_hops` and `error_source` for an
/// `ApiError` with the default `&'static str` code. An error with a typed code
/// enum is not recognised and falls into the generic `request error` arm (only
/// its `Debug`); use [`error_log_for`] for those.
#[inline(always)]
pub fn error_log(err: &actix_web::Error) {
    error_log_for::<&'static str>(err)
}

/// [`debug_log`] for a service whose errors carry the typed code `C`.
///
/// Falls back to the default `ApiError<&'static str>` before the generic arm, so
/// a mixed codebase (library helpers commonly raise the default) keeps
/// structured logs for both. Pass it to
/// [`Logger::with_error_logger`](crate::Logger::with_error_logger) or
/// [`Logger::error_logger`](crate::Logger::error_logger).
#[inline(always)]
pub fn debug_log_for<C: treat_core::ApiErrorCode + 'static>(err: &actix_web::Error) {
    let logged = log_api_error::<C>(err, tracing::Level::DEBUG);
    if logged {
        return;
    }
    let logged = log_api_error::<&'static str>(err, tracing::Level::DEBUG);
    if logged {
        return;
    }

    tracing::debug!(error = ?err.as_response_error(), "request error");
}

/// [`error_log`] for a service whose errors carry the typed code `C`. See
/// [`debug_log_for`].
#[inline(always)]
pub fn error_log_for<C: treat_core::ApiErrorCode + 'static>(err: &actix_web::Error) {
    let logged = log_api_error::<C>(err, tracing::Level::ERROR);
    if logged {
        return;
    }
    let logged = log_api_error::<&'static str>(err, tracing::Level::ERROR);
    if logged {
        return;
    }

    tracing::error!(error = ?err.as_response_error(), "request error");
}
