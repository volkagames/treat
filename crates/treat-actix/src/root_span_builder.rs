use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::{Error, ResponseError, Result};
use tracing::Span;

/// Decides how the [`Logger`](crate::Logger) opens and closes the per-request
/// root span.
///
/// [`DefaultRootSpanBuilder`] is what `Logger::default()` uses. To add fields
/// of your own, implement this trait — usually by calling
/// [`root_span!`](crate::root_span!) with the extra fields and delegating
/// `on_request_end` to [`DefaultRootSpanBuilder`] — and install it with
/// `Logger::<YourBuilder>::new()`.
pub trait RootSpanBuilder {
    /// Open the root span. Called before the inner service runs, after the
    /// [`RequestId`](crate::RequestId) is in the request extensions.
    fn on_request_start(request: &ServiceRequest) -> Span;
    /// Record the outcome on the root span `span`.
    fn on_request_end<B>(span: Span, outcome: &Result<ServiceResponse<B>, Error>);
}

/// The default [`RootSpanBuilder`]: [`root_span!`](crate::root_span!) with no
/// extra fields, and on response:
///
/// - `http.status_code` — the status the client receives,
/// - `otel.status_code` — `ERROR` when that status is a 5xx, `OK` otherwise
///   (OpenTelemetry's server-span rule: a 4xx is the caller's fault, and a
///   `200` that carries an `ApiError` is a handled outcome),
/// - `exception.message` / `exception.details` — the error's `Display` /
///   `Debug`, when the response carries an error.
pub struct DefaultRootSpanBuilder;

impl RootSpanBuilder for DefaultRootSpanBuilder {
    fn on_request_start(request: &ServiceRequest) -> Span {
        crate::root_span!(request)
    }

    fn on_request_end<B>(span: Span, outcome: &Result<ServiceResponse<B>, Error>) {
        match &outcome {
            Ok(response) => {
                // The status already built for the outgoing HTTP response.
                let status = response.status();
                if let Some(error) = response.response().error() {
                    record_exception(&span, error.as_response_error());
                }
                record_status(&span, status);
            }
            Err(error) => {
                let response_error = error.as_response_error();
                record_exception(&span, response_error);
                record_status(&span, response_error.status_code());
            }
        };
    }
}

fn record_exception(span: &Span, response_error: &dyn ResponseError) {
    // pre-formatting errors is a workaround for https://github.com/tokio-rs/tracing/issues/1565
    let display = format!("{response_error}");
    let debug = format!("{response_error:?}");
    span.record("exception.message", tracing::field::display(display));
    span.record("exception.details", tracing::field::display(debug));
}

/// `http.status_code`, and `otel.status_code` = `ERROR` iff the status is a
/// 5xx. The same rule as `treat-tower`'s `record_status`.
fn record_status(span: &Span, status_code: StatusCode) {
    let code: i32 = status_code.as_u16().into();
    span.record("http.status_code", code);
    let otel_status = if status_code.is_server_error() { "ERROR" } else { "OK" };
    span.record("otel.status_code", otel_status);
}
