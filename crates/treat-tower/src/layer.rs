//! The [`tower`] middleware: a [`TraceLayer`] wrapping any HTTP service, adding
//! a per-request [`RequestId`](crate::RequestId), a root span, and (under the
//! `telemetry` feature) OpenTelemetry parent-context propagation.

use crate::RequestId;
use crate::root_span::{record_status, root_span};
use http::{Request, Response};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::{Layer, Service};
use tracing::Span;

/// [`tower::Layer`] that installs the request-id + root-span middleware.
///
/// ```
/// # use treat_tower::TraceLayer;
/// let layer = TraceLayer::new();
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct TraceLayer;

impl TraceLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for TraceLayer {
    type Service = TraceService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TraceService { inner }
    }
}

/// The [`Service`] produced by [`TraceLayer`].
#[derive(Clone, Debug)]
pub struct TraceService<S> {
    inner: S,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for TraceService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = TraceFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        // Adopt the caller's id when present so one identifier spans the whole
        // call chain; otherwise start a new one here.
        let request_id = RequestId::from_headers(req.headers());
        req.extensions_mut().insert(request_id);

        let span = root_span(&req, request_id);

        #[cfg(feature = "telemetry")]
        crate::otel::set_otel_parent(req.headers(), &span);

        // Enter the span while polling the inner service so downstream events
        // are recorded under the root span.
        let future = {
            let _guard = span.enter();
            self.inner.call(req)
        };

        TraceFuture {
            future,
            span,
            request_id,
        }
    }
}

/// Future for [`TraceService`]: records the response status on the span and
/// echoes the request id back to the caller when the inner future resolves.
#[pin_project]
pub struct TraceFuture<F> {
    #[pin]
    future: F,
    span: Span,
    request_id: RequestId,
}

impl<F, ResBody, E> Future for TraceFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = Result<Response<ResBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let _guard = this.span.enter();
        let mut outcome = std::task::ready!(this.future.poll(cx));
        if let Ok(response) = &mut outcome {
            record_status(this.span, response.status());
            // A handler that set the header itself has the final say; only fill in
            // the gap so the caller can always correlate the response with a log.
            response
                .headers_mut()
                .entry(crate::X_REQUEST_ID)
                .or_insert_with(|| this.request_id.to_header_value());
        }
        Poll::Ready(outcome)
    }
}
