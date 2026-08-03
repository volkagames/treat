use super::{DefaultRootSpanBuilder, RequestId, RootSpan, RootSpanBuilder};
use actix_web::body::{BodySize, MessageBody};
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::web::Bytes;
use actix_web::{Error, HttpMessage};
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::task::{Context, Poll};
use tracing::Span;

#[derive(Clone)]
pub struct Logger<RootSpan: RootSpanBuilder, L: Fn(&actix_web::Error) + Clone> {
    error_handler: L,
    root_span_builder: std::marker::PhantomData<RootSpan>,
}

impl<L: Fn(&actix_web::Error) + Clone> Logger<DefaultRootSpanBuilder, L> {
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

impl<RootSpan: RootSpanBuilder> Logger<RootSpan, fn(&actix_web::Error)> {
    pub fn new() -> Logger<RootSpan, fn(&actix_web::Error)> {
        Logger {
            root_span_builder: Default::default(),
            error_handler: error_log,
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
        req.extensions_mut().insert(RequestId::generate());
        let root_span = RootSpanType::on_request_start(&req);

        let root_span_wrapper = RootSpan::new(root_span.clone());
        req.extensions_mut().insert(root_span_wrapper);

        let fut = root_span.in_scope(|| self.service.call(req));

        TracingResponse {
            fut,
            error_handler: self.error_handler.clone(),
            span: root_span,
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
        let error_handle = self.error_handler.clone();
        let this = self.project();

        let fut = this.fut;
        let span = this.span;

        span.in_scope(|| {
            match fut.poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(outcome) => {
                    RootSpanType::on_request_end(Span::current(), &outcome);

                    match &outcome {
                        Ok(response) => {
                            match response.response().error() {
                                Some(err) => error_handle(err),
                                None => tracing::debug!("request success"),
                            }
                        }
                        Err(err) => error_handle(err),
                    }

                    Poll::Ready(outcome.map(|service_response| {
                        service_response.map_body(|_, body| {
                            StreamSpan {
                                body,
                                span: span.clone(),
                            }
                        })
                    }))
                }
            }
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
/// [`ApiErrorHandler`](leto_core::ApiErrorHandler) either. Hence the explicit
/// type parameter: a service with a typed code enum picks the matching logger
/// with [`error_log_for`] / [`debug_log_for`], which mirrors how
/// `report_collect_messages` takes the caller's `C`.
///
/// Returns `false` when the error is not an `ApiError<C>`, so callers can chain
/// a fallback.
fn log_api_error<C: leto_core::ApiErrorCode + 'static>(err: &actix_web::Error, level: tracing::Level) -> bool {
    let Some(err) = err.as_error::<leto_core::ApiError<C>>() else {
        return false;
    };
    let message = err.format_message().unwrap_or_default();
    // `tracing` needs a const level per call site, so the arms are spelled out.
    match level {
        tracing::Level::ERROR => {
            tracing::error!(error_code = %err.code(), error_source = ?err.source(), %message)
        }
        _ => tracing::debug!(error_code = %err.code(), error_source = ?err.source(), %message),
    }
    true
}

#[inline(always)]
pub fn debug_log(err: &actix_web::Error) {
    debug_log_for::<&'static str>(err)
}

#[inline(always)]
pub fn error_log(err: &actix_web::Error) {
    error_log_for::<&'static str>(err)
}

/// [`debug_log`] for a service whose errors carry the typed code `C`.
///
/// Falls back to the default `ApiError<&'static str>` before the generic arm, so
/// a mixed codebase (library helpers commonly raise the default) keeps
/// structured logs for both. Pass it to
/// [`Logger::with_error_logger`](crate::Logger::with_error_logger).
#[inline(always)]
pub fn debug_log_for<C: leto_core::ApiErrorCode + 'static>(err: &actix_web::Error) {
    if log_api_error::<C>(err, tracing::Level::DEBUG) || log_api_error::<&'static str>(err, tracing::Level::DEBUG) {
        return;
    }

    tracing::debug!(error = ?err.as_response_error(), "request error");
}

/// [`error_log`] for a service whose errors carry the typed code `C`. See
/// [`debug_log_for`].
#[inline(always)]
pub fn error_log_for<C: leto_core::ApiErrorCode + 'static>(err: &actix_web::Error) {
    if log_api_error::<C>(err, tracing::Level::ERROR) || log_api_error::<&'static str>(err, tracing::Level::ERROR) {
        return;
    }

    tracing::error!(error = ?err.as_response_error(), "request error");
}
