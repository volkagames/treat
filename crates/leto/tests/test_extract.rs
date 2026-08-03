//! G2: the `ApiJson<T>` / `ApiValidated<T>` request extractors for axum and
//! actix report parse and validation failures in the `leto` envelope.
#![cfg(feature = "serde-path")]

use leto::ApiResponse;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Body {
    #[allow(dead_code)]
    email: String,
    age: u8,
}

#[cfg(all(feature = "validator-extract", feature = "validator"))]
#[derive(Debug, Deserialize, validator::Validate)]
struct Signup {
    #[validate(email)]
    #[allow(dead_code)]
    email: String,
}

#[cfg(feature = "axum")]
mod axum_extract {
    use super::*;
    use axum::body::Body as AxumBody;
    use axum::extract::FromRequest;
    use axum::http::Request;
    use axum::response::IntoResponse;
    use leto::extract_axum::ApiJson;

    fn json_request(body: &str) -> Request<AxumBody> {
        Request::builder()
            .header("content-type", "application/json")
            .body(AxumBody::from(body.to_owned()))
            .expect("request")
    }

    async fn decode(resp: axum::response::Response) -> ApiResponse {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        serde_json::from_slice(&bytes).expect("leto json")
    }

    #[tokio::test]
    async fn api_json_parses_valid_body() {
        let req = json_request(r#"{"email":"a@b.com","age":30}"#);
        let ApiJson(body) = ApiJson::<Body>::from_request(req, &()).await.expect("extracted");
        assert_eq!(body.age, 30);
    }

    #[tokio::test]
    async fn api_json_reports_pointer_on_bad_body() {
        let req = json_request(r#"{"email":"a@b.com","age":"old"}"#);
        let rejection = ApiJson::<Body>::from_request(req, &()).await.expect_err("rejection");
        let decoded = decode(rejection.into_response()).await;
        assert_eq!(decoded.first_error_code(), Some("invalid_body"));
        assert_eq!(
            decoded.errors[0].source.as_ref().and_then(|s| s.pointer.as_deref()),
            Some("/age"),
        );
    }

    #[cfg(all(feature = "validator-extract", feature = "validator"))]
    #[tokio::test]
    async fn api_validated_reports_field_errors() {
        use leto::extract_axum::ApiValidated;

        let req = json_request(r#"{"email":"nope"}"#);
        let rejection = ApiValidated::<Signup>::from_request(req, &())
            .await
            .expect_err("rejection");
        let decoded = decode(rejection.into_response()).await;
        assert_eq!(decoded.first_error_code(), Some("email"));
        assert_eq!(
            decoded.errors[0].source.as_ref().and_then(|s| s.pointer.as_deref()),
            Some("/email"),
        );
    }
}

#[cfg(feature = "actix")]
mod actix_extract {
    use super::*;
    use actix_web::body::MessageBody;
    use actix_web::{FromRequest, ResponseError, test};
    use leto::extract_actix::ApiJson;

    #[actix_web::test]
    async fn api_json_parses_valid_body() {
        let (req, mut payload) = test::TestRequest::default()
            .set_payload(r#"{"email":"a@b.com","age":30}"#)
            .to_http_parts();
        let ApiJson(body) = ApiJson::<Body>::from_request(&req, &mut payload)
            .await
            .expect("extracted");
        assert_eq!(body.age, 30);
    }

    #[actix_web::test]
    async fn api_json_reports_pointer_on_bad_body() {
        let (req, mut payload) = test::TestRequest::default()
            .set_payload(r#"{"email":"a@b.com","age":"old"}"#)
            .to_http_parts();
        let err = ApiJson::<Body>::from_request(&req, &mut payload)
            .await
            .expect_err("rejection");
        let resp = err.error_response();
        let bytes = resp.into_body().try_into_bytes().expect("body");
        let decoded: ApiResponse = serde_json::from_slice(&bytes).expect("leto json");
        assert_eq!(decoded.first_error_code(), Some("invalid_body"));
        assert_eq!(
            decoded.errors[0].source.as_ref().and_then(|s| s.pointer.as_deref()),
            Some("/age"),
        );
    }

    #[cfg(all(feature = "validator-extract", feature = "validator"))]
    #[actix_web::test]
    async fn api_validated_reports_field_errors() {
        use leto::extract_actix::ApiValidated;

        let (req, mut payload) = test::TestRequest::default()
            .set_payload(r#"{"email":"nope"}"#)
            .to_http_parts();
        let err = ApiValidated::<Signup>::from_request(&req, &mut payload)
            .await
            .expect_err("rejection");
        let resp = err.error_response();
        let bytes = resp.into_body().try_into_bytes().expect("body");
        let decoded: ApiResponse = serde_json::from_slice(&bytes).expect("leto json");
        assert_eq!(decoded.first_error_code(), Some("email"));
    }
}
