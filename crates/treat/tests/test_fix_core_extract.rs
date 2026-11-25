//! Regressions for the request extractors' failure responses.
//!
//! - A body that cannot be read at all (over the size limit) is an
//!   `invalid_body` envelope that keeps the framework's status (`413`), on both
//!   frameworks and for both extractors — not axum's plain-text rejection, and
//!   not actix's status replaced by `422`.
//! - actix's `ApiValidated` keeps a parse failure's `ApiError` whole, so the
//!   error is stashed for `response_get_api_error_actix`, and its responses
//!   carry `X-RPC-Status` under `rpc-status-header`, as axum's do.
#![cfg(all(
    any(feature = "serde-path", feature = "validator-extract"),
    any(feature = "axum", feature = "actix")
))]

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "validator-extract", derive(validator::Validate))]
struct Body {
    #[cfg_attr(feature = "validator-extract", validate(email))]
    email: String,
}

/// A body well over the limit the tests configure; the other bodies fit in it.
const LARGE: &str = r#"{"email":"someone-with-a-long-name@example.com"}"#;
const LIMIT: usize = 24;

#[cfg(feature = "axum")]
mod axum_extract {
    use super::*;
    use axum::Router;
    use axum::body::Body as AxumBody;
    use axum::extract::DefaultBodyLimit;
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use tower::ServiceExt;
    use treat::ApiResponse;

    async fn post_json(router: Router, body: &str) -> (StatusCode, axum::http::HeaderMap, ApiResponse) {
        let request = Request::builder()
            .method("POST")
            .uri("/")
            .header("content-type", "application/json")
            .body(AxumBody::from(body.to_owned()))
            .expect("request");
        let response = router.oneshot(request).await.expect("infallible");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let decoded = serde_json::from_slice(&bytes)
            .unwrap_or_else(|err| panic!("not a treat envelope ({err}): {}", String::from_utf8_lossy(&bytes)));
        (status, headers, decoded)
    }

    #[tokio::test]
    async fn api_json_reports_an_oversized_body_as_invalid_body_at_413() {
        async fn handler(treat::extract_axum::ApiJson(body): treat::extract_axum::ApiJson<Body>) -> String {
            body.email
        }
        let router = Router::new()
            .route("/", post(handler))
            .layer(DefaultBodyLimit::max(LIMIT));
        let (status, headers, decoded) = post_json(router, LARGE).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(decoded.first_error_code(), Some("invalid_body"));
        #[cfg(feature = "rpc-status-header")]
        assert_eq!(
            headers
                .get(treat::rpc_status::X_RPC_STATUS)
                .and_then(|v| v.to_str().ok()),
            Some(treat::rpc_status::ERROR)
        );
        let _ = headers;
    }

    #[cfg(feature = "validator-extract")]
    #[tokio::test]
    async fn api_validated_reports_an_oversized_body_as_invalid_body_at_413() {
        async fn handler(treat::extract_axum::ApiValidated(body): treat::extract_axum::ApiValidated<Body>) -> String {
            body.into_inner().email
        }
        let router = Router::new()
            .route("/", post(handler))
            .layer(DefaultBodyLimit::max(LIMIT));
        let (status, _, decoded) = post_json(router, LARGE).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(decoded.first_error_code(), Some("invalid_body"));
    }
}

#[cfg(feature = "actix")]
mod actix_extract {
    use super::*;
    use actix_web::http::StatusCode;
    use actix_web::{App, test as actix_test, web};
    use treat::ApiResponse;

    async fn post_json<F, Args>(handler: F, body: &str) -> (StatusCode, actix_web::dev::ServiceResponse)
    where
        F: actix_web::Handler<Args>,
        Args: actix_web::FromRequest + 'static,
        F::Output: actix_web::Responder + 'static,
    {
        let app = actix_test::init_service(
            App::new()
                .app_data(web::PayloadConfig::new(LIMIT))
                .route("/", web::post().to(handler)),
        )
        .await;
        let request = actix_test::TestRequest::post()
            .uri("/")
            .insert_header(("content-type", "application/json"))
            .set_payload(body.to_owned())
            .to_request();
        let response = actix_test::call_service(&app, request).await;
        (response.status(), response)
    }

    fn decode(bytes: &[u8]) -> ApiResponse {
        serde_json::from_slice(bytes)
            .unwrap_or_else(|err| panic!("not a treat envelope ({err}): {}", String::from_utf8_lossy(bytes)))
    }

    async fn api_json(treat::extract_actix::ApiJson(body): treat::extract_actix::ApiJson<Body>) -> String {
        body.email
    }

    #[actix_web::test]
    async fn api_json_reports_an_oversized_body_as_invalid_body_at_413() {
        let (status, response) = post_json(api_json, LARGE).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        let decoded = decode(&actix_test::read_body(response).await);
        assert_eq!(decoded.first_error_code(), Some("invalid_body"));
    }

    #[cfg(feature = "validator-extract")]
    async fn api_validated(
        treat::extract_actix::ApiValidated(body): treat::extract_actix::ApiValidated<Body>,
    ) -> String {
        body.into_inner().email
    }

    #[cfg(feature = "validator-extract")]
    #[actix_web::test]
    async fn api_validated_reports_an_oversized_body_as_invalid_body_at_413() {
        let (status, response) = post_json(api_validated, LARGE).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        let stashed = treat::response_get_api_error_actix(response.response()).expect("the error is stashed");
        assert_eq!(stashed.code(), "invalid_body");
        assert_eq!(stashed.status(), 413);
        #[cfg(feature = "rpc-status-header")]
        assert_eq!(
            response
                .headers()
                .get(treat::rpc_status::X_RPC_STATUS)
                .and_then(|v| v.to_str().ok()),
            Some(treat::rpc_status::ERROR)
        );
        let decoded = decode(&actix_test::read_body(response).await);
        assert_eq!(decoded.first_error_code(), Some("invalid_body"));
    }

    /// A parse failure renders through `ApiError`'s own adapter: the error is
    /// stashed for the loggers and keeps its pointer and `422`.
    #[cfg(feature = "validator-extract")]
    #[actix_web::test]
    async fn api_validated_keeps_a_parse_error_whole() {
        let (status, response) = post_json(api_validated, r#"{"email":1}"#).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let stashed = treat::response_get_api_error_actix(response.response()).expect("the error is stashed");
        assert_eq!(stashed.code(), "invalid_body");
        assert_eq!(
            stashed.error_source().and_then(|source| source.pointer.as_deref()),
            Some("/email")
        );
        #[cfg(feature = "rpc-status-header")]
        assert_eq!(
            response
                .headers()
                .get(treat::rpc_status::X_RPC_STATUS)
                .and_then(|v| v.to_str().ok()),
            Some(treat::rpc_status::ERROR)
        );
    }

    /// A validation failure is a multi-error envelope at `422`, with the header.
    #[cfg(feature = "validator-extract")]
    #[actix_web::test]
    async fn api_validated_answers_a_validation_failure_with_the_header() {
        let (status, response) = post_json(api_validated, r#"{"email":"nope"}"#).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        #[cfg(feature = "rpc-status-header")]
        assert_eq!(
            response
                .headers()
                .get(treat::rpc_status::X_RPC_STATUS)
                .and_then(|v| v.to_str().ok()),
            Some(treat::rpc_status::ERROR)
        );
        let decoded = decode(&actix_test::read_body(response).await);
        assert_eq!(decoded.first_error_code(), Some("email"));
    }

    #[cfg(feature = "validator-extract")]
    #[test]
    fn with_status_overrides_either_kind_of_rejection() {
        use actix_web::ResponseError;
        use treat::extract_actix::ApiResponseError;

        let envelope = ApiResponseError::new(treat::failure([])).with_status(400);
        assert_eq!(envelope.status_code(), StatusCode::BAD_REQUEST);
        let error = ApiResponseError::from(treat::error("invalid_body")).with_status(413);
        assert_eq!(error.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[cfg(all(feature = "validator-extract", debug_assertions))]
    #[test]
    #[should_panic(expected = "invalid HTTP status")]
    fn with_status_refuses_an_invalid_status_in_debug() {
        let _ = treat::extract_actix::ApiResponseError::new(treat::failure([])).with_status(42);
    }
}
