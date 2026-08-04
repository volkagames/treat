use crate::*;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::{Error, ResponseError, Result};
use tracing::Span;

pub trait RootSpanBuilder {
    fn on_request_start(request: &ServiceRequest) -> Span;
    fn on_request_end<B>(span: Span, outcome: &Result<ServiceResponse<B>, Error>);
}

pub struct DefaultRootSpanBuilder;

impl RootSpanBuilder for DefaultRootSpanBuilder {
    fn on_request_start(request: &ServiceRequest) -> Span {
        root_span!(request)
    }

    fn on_request_end<B>(span: Span, outcome: &Result<ServiceResponse<B>, Error>) {
        match &outcome {
            Ok(response) => {
                if let Some(error) = response.response().error() {
                    // use the status code already constructed for the outgoing HTTP response
                    handle_error(span, response.status(), error.as_response_error());
                } else {
                    let code: i32 = response.response().status().as_u16().into();
                    span.record("http.status_code", code);
                    span.record("otel.status_code", "OK");
                }
            }
            Err(error) => {
                let response_error = error.as_response_error();
                handle_error(span, response_error.status_code(), response_error);
            }
        };
    }
}

fn handle_error(span: Span, status_code: StatusCode, response_error: &dyn ResponseError) {
    // pre-formatting errors is a workaround for https://github.com/tokio-rs/tracing/issues/1565
    let display = format!("{response_error}");
    let debug = format!("{response_error:?}");
    span.record("exception.message", tracing::field::display(display));
    span.record("exception.details", tracing::field::display(debug));
    let code: i32 = status_code.as_u16().into();

    span.record("http.status_code", code);

    if status_code.is_client_error() {
        span.record("otel.status_code", "OK");
    } else {
        span.record("otel.status_code", "ERROR");
    }
}
