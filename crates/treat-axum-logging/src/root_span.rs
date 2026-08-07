//! The per-request root span, built from `http` types with a compact
//! `request_*` / `http_*` field set.
//!
//! [`LoggingLayer`](crate::LoggingLayer) opens this span in `call`, before the
//! inner service runs, so tracking covers the whole request rather than starting
//! once a response exists. The functions are public so a service that builds its
//! own middleware can reuse the same field set:
//!
//! ```
//! use tracing::Instrument;
//! # use treat_axum_logging::{root_span_on_request_start, root_span_on_response_end};
//! # async fn call(_: http::Request<()>) -> http::Response<String> { unimplemented!() }
//! # async fn example(request: http::Request<()>) {
//! let span = root_span_on_request_start(&request);
//! let response = async move {
//!     let response = call(request).await;
//!     root_span_on_response_end(&tracing::Span::current(), response)
//! }
//! .instrument(span)
//! .await;
//! # }
//! ```

use http::Request;
use http_body::Body;
use tracing::Span;
use treat_tower::RequestId;

/// Open the per-request root span, recording what is known before the inner
/// service runs. `http_status_code` and `http_body_size` are left empty and
/// filled in by [`root_span_on_response_end`].
///
/// The span is opened at [`Level::ERROR`](tracing::Level::ERROR) deliberately: a
/// span's level decides whether it is created at all, and these fields have to
/// survive to annotate an error logged at the end of the request — including
/// under a filter as coarse as `RUST_LOG=error`.
///
/// `request_id` is recorded only when something upstream (e.g.
/// [`treat_tower::TraceLayer`]) has already put one in the request extensions.
///
/// [`treat_tower::TraceLayer`]: https://docs.rs/treat-tower
pub fn root_span_on_request_start<ReqBody>(request: &Request<ReqBody>) -> Span {
    let span = tracing::span!(
        tracing::Level::ERROR,
        "request",
        request_method = %request.method(),
        request_uri = %request.uri(),
        request_id = tracing::field::Empty,
        http_body_size = tracing::field::Empty,
        http_client_ip = tracing::field::Empty,
        http_status_code = tracing::field::Empty,
        http_user_agent = tracing::field::Empty,
    );

    record_request_id(&span, request);
    record_header_field(&span, request, http::header::USER_AGENT, "http_user_agent");
    record_header_field(
        &span,
        request,
        http::HeaderName::from_static("x-forwarded-for"),
        "http_client_ip",
    );

    span
}

/// Record the [`RequestId`] an upstream [`TraceLayer`] stored in the request
/// extensions. A no-op when no such layer ran — this crate never generates an id
/// of its own, so the field simply stays empty.
///
/// [`TraceLayer`]: treat_tower::TraceLayer
pub fn record_request_id<ReqBody>(span: &Span, request: &Request<ReqBody>) {
    if let Some(value) = request.extensions().get::<RequestId>() {
        span.record("request_id", value.to_string());
    }
}

fn record_header_field<ReqBody>(
    span: &Span,
    request: &Request<ReqBody>,
    header_name: http::HeaderName,
    field_name: &'static str,
) {
    if let Some(value) = request.headers().get(&header_name) {
        span.record(field_name, value.to_str().unwrap_or("[non-UTF8]"));
    }
}

/// Record what only the response knows — status, and the body size when it is
/// known exactly — and hand the response back unchanged.
pub fn root_span_on_response_end<ResBody: Body>(
    span: &Span,
    response: http::Response<ResBody>,
) -> http::Response<ResBody> {
    span.record("http_status_code", response.status().as_u16());
    if let Some(body_size) = response.body().size_hint().exact() {
        span.record("http_body_size", body_size);
    }
    response
}
