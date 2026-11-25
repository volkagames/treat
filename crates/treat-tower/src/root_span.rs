//! Build the per-request root span. Mirrors the field set of the actix
//! middleware (`treat-actix`) so traces look the same across frameworks, but
//! reads everything from `http` types instead of actix's `ServiceRequest`.
//!
//! Where the two still differ (see `docs/frameworks.md`):
//!
//! - `http.route` (and so `otel.name`) is the raw request path, not the matched
//!   route pattern: tower has no router, and axum's `MatchedPath` is an axum
//!   type this crate does not depend on. A path with ids in it makes
//!   `otel.name` high-cardinality.
//! - `http.host` / `http.scheme` prefer the URI authority / scheme over the
//!   `Host` / `X-Forwarded-Proto` headers and do not read `Forwarded` or
//!   `X-Forwarded-Host`; actix prefers the forwarded headers.
//! - `http.client_ip` falls back to a [`SocketAddr`] in the request
//!   extensions, which only exists if something put one there (axum's
//!   `ConnectInfo<SocketAddr>` is a different type and is not read).

use crate::RequestId;
use http::{HeaderMap, Method, Request, Version};
use std::borrow::Cow;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::Span;
use treat_core::ApiErrorHandler;

/// Open the "HTTP request" root span for `request`, recording the standard
/// OpenTelemetry-flavoured HTTP fields. `http.status_code` /
/// `otel.status_code` / `trace_id` / `exception.*` are left empty and filled
/// in on response.
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
    let client_ip = request_client_ip(request);

    tracing::info_span!(
        "HTTP request",
        http.method = %method,
        http.route = %route,
        http.flavor = %http_flavor(request.version()),
        http.scheme = %scheme,
        http.host = %host,
        http.client_ip = %client_ip,
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
/// pseudo-header, so the URI is tried first.
pub fn request_host<B>(request: &Request<B>) -> &str {
    request
        .uri()
        .host()
        .or_else(|| request.headers().get(http::header::HOST).and_then(|h| h.to_str().ok()))
        .unwrap_or("")
}

/// Resolve the request scheme for the span: the URI scheme (HTTP/2), else the
/// `X-Forwarded-Proto` header (set by reverse proxies, but also by any client
/// that sends it), else `http`, like actix's `connection_info`.
pub fn request_scheme<B>(request: &Request<B>) -> &str {
    request
        .uri()
        .scheme_str()
        .or_else(|| request.headers().get("x-forwarded-proto").and_then(|h| h.to_str().ok()))
        .unwrap_or("http")
}

/// Resolve the client address for the span, in the order actix's
/// `realip_remote_addr` uses: the first `for=` of a `Forwarded` header, else
/// the first `X-Forwarded-For` entry, else the IP of a [`SocketAddr`] in the
/// request extensions, else empty.
///
/// The headers are whatever the client (or each proxy) sent, so the value is
/// spoofable unless a trusted proxy overwrites them. Log it; do not authorize
/// on it.
pub fn request_client_ip<B>(request: &Request<B>) -> Cow<'_, str> {
    let headers = request.headers();
    let forwarded = forwarded_for(headers).or_else(|| first_header_value(headers, "x-forwarded-for"));
    match forwarded {
        Some(ip) => Cow::Borrowed(ip),
        None => {
            request
                .extensions()
                .get::<SocketAddr>()
                .map_or(Cow::Borrowed(""), |addr| Cow::Owned(addr.ip().to_string()))
        }
    }
}

/// The first `for=` value of the `Forwarded` headers (RFC 7239), unquoted and
/// without port or IPv6 brackets.
fn forwarded_for(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(http::header::FORWARDED)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split([';', ',']))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("for"))
        .map(|(_, value)| bare_address(value.trim().trim_matches('"')))
}

/// Strip the port and IPv6 square brackets from a `Forwarded` node.
fn bare_address(value: &str) -> &str {
    match value.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(rest),
        None => value.split(':').next().unwrap_or(value),
    }
}

fn first_header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let value = headers.get(name)?.to_str().ok()?;
    value.split(',').next().map(str::trim)
}

/// Record the response status on the span: `http.status_code`, and
/// `otel.status_code` = `ERROR` iff the status is a 5xx (OpenTelemetry's rule
/// for server spans), `OK` otherwise. The actix builder uses the same rule.
pub fn record_status(span: &Span, status: http::StatusCode) {
    let code: i32 = status.as_u16().into();
    span.record("http.status_code", code);
    let otel_status = if status.is_server_error() { "ERROR" } else { "OK" };
    span.record("otel.status_code", otel_status);
}

/// Record the `treat` error a response carries as `exception.message` (the
/// same text as the error's `Display`: `treat error: <code>[, message: <msg>]`)
/// and `exception.details` (its `Debug`). The error is the
/// `Arc<dyn ApiErrorHandler>` treat's axum adapter stores in the response
/// extensions; a response without one leaves both fields empty.
pub fn record_api_error<B>(span: &Span, response: &http::Response<B>) {
    let Some(error) = response.extensions().get::<Arc<dyn ApiErrorHandler>>() else {
        return;
    };
    let message = error.format_message().filter(|message| !message.is_empty());
    let display = match message {
        Some(message) => format!("treat error: {}, message: {message}", error.code()),
        None => format!("treat error: {}", error.code()),
    };
    let details = format!("{error:?}");
    span.record("exception.message", tracing::field::display(display));
    span.record("exception.details", tracing::field::display(details));
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
