//! The actix `Logger` adopts a caller-supplied `x-request-id` and echoes it back
//! on the response, mirroring the tower `TraceLayer`. Only a UUID is adopted, so
//! a caller cannot inject arbitrary text into the `request_id` span field.
#![cfg(feature = "actix")]

use actix_web::{App, HttpResponse, test, web};
use treat::{Logger, RequestId, X_REQUEST_ID};

async fn handler(id: RequestId) -> HttpResponse {
    HttpResponse::Ok().body(id.to_string())
}

/// Sends one request through the logger, returning the id the handler saw and the
/// id echoed on the response header.
async fn call_with(header: Option<&str>) -> (String, String) {
    let app = test::init_service(App::new().wrap(Logger::default()).route("/", web::get().to(handler))).await;

    let mut request = test::TestRequest::get().uri("/");
    if let Some(header) = header {
        request = request.insert_header((X_REQUEST_ID, header));
    }

    let response = test::call_service(&app, request.to_request()).await;
    assert!(response.status().is_success());

    let echoed = response
        .headers()
        .get(X_REQUEST_ID)
        .expect("the logger must echo the request id")
        .to_str()
        .expect("ascii header")
        .to_owned();

    let body = test::read_body(response).await;
    let seen = String::from_utf8(body.to_vec()).expect("utf8");

    (seen, echoed)
}

#[actix_web::test]
async fn generated_id_is_echoed_on_the_response() {
    let (seen, echoed) = call_with(None).await;

    assert_eq!(seen.len(), 36, "a v4 UUID renders as 36 chars");
    assert_eq!(echoed, seen, "the echoed header must match the handler's id");
}

#[actix_web::test]
async fn caller_supplied_uuid_is_adopted() {
    let supplied = "67e55044-10b1-426f-9247-bb680e5fe0c8";

    let (seen, echoed) = call_with(Some(supplied)).await;

    assert_eq!(seen, supplied, "the caller's id must reach the handler");
    assert_eq!(echoed, supplied, "a canonical id is echoed back byte for byte");
}

/// A non-canonical UUID is adopted but re-emitted canonically. Normalizing is the
/// point: every service on the path then logs and forwards the same lowercase
/// hyphenated string, so one id format holds end to end.
#[actix_web::test]
async fn non_canonical_uuid_is_normalized() {
    const CANONICAL: &str = "67e55044-10b1-426f-9247-bb680e5fe0c8";

    for supplied in [
        "{67e55044-10b1-426f-9247-bb680e5fe0c8}",
        "urn:uuid:67e55044-10b1-426f-9247-bb680e5fe0c8",
        "67e5504410b1426f9247bb680e5fe0c8",
        "67E55044-10B1-426F-9247-BB680E5FE0C8",
    ] {
        let (seen, echoed) = call_with(Some(supplied)).await;

        assert_eq!(seen, CANONICAL, "{supplied:?} must reach the handler canonically");
        assert_eq!(echoed, CANONICAL, "{supplied:?} must be echoed canonically");
    }
}

#[actix_web::test]
async fn non_uuid_header_is_ignored() {
    for supplied in ["not-a-uuid", "", "   ", "'; DROP TABLE users --"] {
        let (seen, echoed) = call_with(Some(supplied)).await;

        assert_ne!(seen, supplied, "{supplied:?} must not be adopted");
        assert_eq!(seen.len(), 36, "a fresh UUID replaces {supplied:?}");
        assert_eq!(echoed, seen);
    }
}

#[actix_web::test]
async fn surrounding_whitespace_is_trimmed() {
    let (seen, _) = call_with(Some("  67e55044-10b1-426f-9247-bb680e5fe0c8  ")).await;

    assert_eq!(seen, "67e55044-10b1-426f-9247-bb680e5fe0c8");
}

/// A handler that sets the header itself owns the value; the logger must not
/// overwrite it or append a second one.
#[actix_web::test]
async fn handler_set_header_is_preserved() {
    async fn sets_header() -> HttpResponse {
        HttpResponse::Ok()
            .insert_header((X_REQUEST_ID, "chosen-by-handler"))
            .finish()
    }

    let app = test::init_service(
        App::new()
            .wrap(Logger::default())
            .route("/", web::get().to(sets_header)),
    )
    .await;

    let response = test::call_service(&app, test::TestRequest::get().uri("/").to_request()).await;

    let values: Vec<_> = response.headers().get_all(X_REQUEST_ID).collect();
    assert_eq!(values.len(), 1, "the logger must not append a second value");
    assert_eq!(values[0], "chosen-by-handler");
}
