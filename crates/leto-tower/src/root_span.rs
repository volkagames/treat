//! Build the per-request root span. Mirrors the field set of the actix
//! middleware (`leto-actix`) so traces look the same across frameworks, but
//! reads everything from `http` types instead of actix's `ServiceRequest`.

use crate::RequestId;
use http::{Method, Request, Version};
use std::borrow::Cow;
use tracing::Span;

/// Open the "HTTP request" root span for `request`, recording the standard
/// OpenTelemetry-flavoured HTTP fields. `status_code` / `otel.status_code` /
/// `trace_id` / `exception.*` are left empty and filled in on response.
pub fn root_span<B>(request: &Request<B>, request_id: RequestId) -> Span {
    let method = http_method_str(request.method());
    // tower/axum resolve the matched route only after routing; use the path here.
    let route: Cow<'static, str> = request.uri().path().to_owned().into();
    let user_agent = request
        .headers()
        .get(http::header::USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let target = request.uri().path_and_query().map(|p| p.as_str()).unwrap_or("");
    let host = request_host(request);
    let scheme = request_scheme(request);

    tracing::info_span!(
        "HTTP request",
        http.method = %method,
        http.route = %route,
        http.flavor = %http_flavor(request.version()),
        http.scheme = %scheme,
        http.host = %host,
        http.user_agent = %user_agent,
        http.target = %target,
        http.status_code = tracing::field::Empty,
        otel.name = %format!("HTTP {method} {route}"),
        otel.kind = "server",
        otel.status_code = tracing::field::Empty,
        trace_id = tracing::field::Empty,
        request_id = %request_id,
        exception.message = tracing::field::Empty,
        exception.details = tracing::field::Empty,
    )
}

/// Resolve the request host for the span. Server-side requests arrive in
/// origin-form (`/path`), so `uri().host()` is `None` on HTTP/1.1 — fall back to
/// the `Host` header. HTTP/2 populates the URI authority from the `:authority`
/// pseudo-header, so the URI is tried first. Mirrors actix's `connection_info`.
pub fn request_host<B>(request: &Request<B>) -> &str {
    request
        .uri()
        .host()
        .or_else(|| request.headers().get(http::header::HOST).and_then(|h| h.to_str().ok()))
        .unwrap_or("")
}

/// Resolve the request scheme for the span: the URI scheme (HTTP/2), else the
/// `X-Forwarded-Proto` header (set by reverse proxies), else empty.
pub fn request_scheme<B>(request: &Request<B>) -> &str {
    request
        .uri()
        .scheme_str()
        .or_else(|| request.headers().get("x-forwarded-proto").and_then(|h| h.to_str().ok()))
        .unwrap_or("")
}

/// Record the response status on the span, matching the actix builder's
/// `otel.status_code` policy (client errors are still `OK` at the span level).
pub fn record_status(span: &Span, status: http::StatusCode) {
    let code: i32 = status.as_u16().into();
    span.record("http.status_code", code);
    if status.is_server_error() {
        span.record("otel.status_code", "ERROR");
    } else {
        span.record("otel.status_code", "OK");
    }
}

fn http_method_str(method: &Method) -> Cow<'static, str> {
    match *method {
        Method::OPTIONS => "OPTIONS".into(),
        Method::GET => "GET".into(),
        Method::POST => "POST".into(),
        Method::PUT => "PUT".into(),
        Method::DELETE => "DELETE".into(),
        Method::HEAD => "HEAD".into(),
        Method::TRACE => "TRACE".into(),
        Method::CONNECT => "CONNECT".into(),
        Method::PATCH => "PATCH".into(),
        _ => method.to_string().into(),
    }
}

fn http_flavor(version: Version) -> Cow<'static, str> {
    match version {
        Version::HTTP_09 => "0.9".into(),
        Version::HTTP_10 => "1.0".into(),
        Version::HTTP_11 => "1.1".into(),
        Version::HTTP_2 => "2.0".into(),
        Version::HTTP_3 => "3.0".into(),
        other => format!("{other:?}").into(),
    }
}
