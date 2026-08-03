//! The `rpc-status-header` feature adds an out-of-band `X-RPC-Status: ok|error`
//! signal, so a caller can tell success from failure without parsing the body.
//!
//! The header tracks `errors[]`, never the status line — which by default is
//! `200 OK` even for a failure. These tests pin that: the same envelope keeps the
//! same header whatever the transport status does.

#![cfg(feature = "rpc-status-header")]

use leto::rpc_status::{ERROR, OK, X_RPC_STATUS};
use leto::{ApiResponse, NoData, error, failure, success};

/// The whole point of the feature: `ok` and `error` are distinguishable, and
/// neither is the empty string a stripped header would look like.
#[test]
fn the_two_outcomes_are_distinct() {
    assert_eq!(X_RPC_STATUS, "x-rpc-status");
    assert_eq!(OK, "ok");
    assert_eq!(ERROR, "error");
    assert_ne!(OK, ERROR);
}

#[test]
fn the_envelope_reports_its_outcome_from_the_errors_field() {
    assert_eq!(success(1_i32).rpc_status(), OK);
    assert_eq!(
        failure::<NoData, ()>([error("boom").into_error_message()]).rpc_status(),
        ERROR
    );
}

/// A success envelope whose `data` is absent is still a success — the header keys
/// on `errors[]`, not on whether `data` happens to be `None`.
#[test]
fn a_null_data_success_is_still_ok() {
    let response: ApiResponse<i32> = ApiResponse {
        data: None,
        meta: None,
        errors: vec![],
    };
    assert_eq!(response.rpc_status(), OK);
}

/// `ApiError` -> envelope always yields an error document, so always `error`.
#[test]
fn a_converted_api_error_reports_error() {
    let response: ApiResponse = error("boom").with_message("bad").into_api_response();
    assert_eq!(response.rpc_status(), ERROR);
}

#[cfg(feature = "axum")]
mod axum_adapter {
    use super::*;
    use axum::response::IntoResponse;

    fn header(response: &axum::response::Response) -> Option<&str> {
        response.headers().get(X_RPC_STATUS)?.to_str().ok()
    }

    #[test]
    fn an_error_response_is_marked_error() {
        assert_eq!(header(&error("boom").into_response()), Some(ERROR));
    }

    #[test]
    fn a_success_response_is_marked_ok() {
        assert_eq!(header(&success(1_i32).into_response()), Some(OK));
    }

    /// The regression this feature exists to prevent: an envelope carrying errors
    /// goes out with `200 OK` on the status line, so only the header reveals it.
    #[test]
    fn a_failure_envelope_on_a_200_is_still_marked_error() {
        let response = failure::<NoData, ()>([error("boom").into_error_message()]).into_response();
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(header(&response), Some(ERROR));
    }

    /// An explicit transport status must not change the outcome signal.
    #[test]
    fn the_header_is_independent_of_the_status_line() {
        let response = error("not_found").with_status(404).into_response();
        assert_eq!(response.status().as_u16(), 404);
        assert_eq!(header(&response), Some(ERROR));
    }

    /// Exactly one value — a duplicated header would be ambiguous to a client.
    #[test]
    fn the_header_is_set_once() {
        let response = error("boom").into_response();
        assert_eq!(response.headers().get_all(X_RPC_STATUS).iter().count(), 1);
    }
}

#[cfg(feature = "actix")]
mod actix_adapter {
    use super::*;
    use actix_web::ResponseError;
    use actix_web::body::BoxBody;

    fn header(response: &actix_web::HttpResponse<BoxBody>) -> Option<&str> {
        response.headers().get(X_RPC_STATUS)?.to_str().ok()
    }

    #[test]
    fn an_error_response_is_marked_error() {
        assert_eq!(header(&error("boom").error_response()), Some(ERROR));
    }

    #[test]
    fn a_success_response_is_marked_ok() {
        use actix_web::Responder;
        let request = actix_web::test::TestRequest::default().to_http_request();
        assert_eq!(header(&success(1_i32).respond_to(&request)), Some(OK));
    }

    #[test]
    fn a_failure_envelope_on_a_200_is_still_marked_error() {
        use actix_web::Responder;
        let request = actix_web::test::TestRequest::default().to_http_request();
        let response = failure::<NoData, ()>([error("boom").into_error_message()]).respond_to(&request);
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(header(&response), Some(ERROR));
    }

    #[test]
    fn the_header_is_independent_of_the_status_line() {
        let response = error("not_found").with_status(404).error_response();
        assert_eq!(response.status().as_u16(), 404);
        assert_eq!(header(&response), Some(ERROR));
    }
}

#[cfg(feature = "poem")]
mod poem_adapter {
    use super::*;
    use poem::IntoResponse;
    use poem::error::ResponseError;

    fn header(response: &poem::Response) -> Option<&str> {
        response.headers().get(X_RPC_STATUS)?.to_str().ok()
    }

    #[test]
    fn an_error_response_is_marked_error() {
        assert_eq!(header(&error("boom").as_response()), Some(ERROR));
    }

    #[test]
    fn a_success_response_is_marked_ok() {
        assert_eq!(header(&success(1_i32).into_response()), Some(OK));
    }

    #[test]
    fn a_failure_envelope_on_a_200_is_still_marked_error() {
        let response = failure::<NoData, ()>([error("boom").into_error_message()]).into_response();
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(header(&response), Some(ERROR));
    }

    #[test]
    fn the_header_is_independent_of_the_status_line() {
        let response = error("not_found").with_status(404).as_response();
        assert_eq!(response.status().as_u16(), 404);
        assert_eq!(header(&response), Some(ERROR));
    }
}
