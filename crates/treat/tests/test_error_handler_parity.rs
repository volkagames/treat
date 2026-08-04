//! Every framework adapter must stash the type-erased error in the response
//! extensions, and [`ApiErrorHandler`] must expose enough of it to be useful to
//! middleware — the transport `status` above all, which observability needs to
//! log what the client actually saw.
//!
//! The accessors differ per framework only because the response types do:
//! `HttpResponse` is not an `http::Response`, and actix hands out its extensions
//! behind a `Ref` guard.

use treat::error;

/// Fields exposed through the erased trait, read the same way for every adapter.
fn assert_handler_fields(handler: &dyn treat::ApiErrorHandler) {
    assert_eq!(handler.code(), "boom");
    assert_eq!(handler.status(), 404);
    assert!(handler.has_status(), "an explicit status must report has_status");
    assert_eq!(
        handler.error_source().and_then(|s| s.pointer.as_deref()),
        Some("/email"),
        "the locator must survive erasure",
    );
    assert!(
        handler.location().file().contains("test_error_handler_parity"),
        "location must point at the raise site, got {}",
        handler.location(),
    );
}

fn raise() -> treat::ApiError {
    error("boom").with_message("m").with_status(404).with_pointer("/email")
}

#[cfg(feature = "axum")]
#[test]
fn axum_stashes_the_error() {
    use axum::response::IntoResponse;

    let response = raise().into_response();
    let handler = treat::response_get_api_error(&response).expect("handler in extensions");
    assert_handler_fields(handler.as_ref());
}

#[cfg(feature = "actix")]
#[test]
fn actix_stashes_the_error() {
    use actix_web::ResponseError;

    let response = raise().error_response();
    let handler = treat::response_get_api_error_actix(&response).expect("handler in extensions");
    assert_handler_fields(handler.as_ref());
}

#[cfg(feature = "poem")]
#[test]
fn poem_stashes_the_error() {
    use poem::error::ResponseError;

    let response = raise().as_response();
    let handler = treat::response_get_api_error_poem(&response).expect("handler in extensions");
    assert_handler_fields(handler.as_ref());
}

/// The direct `error_response()` call above bypasses routing; middleware sees
/// the error only after it has travelled the service pipeline.
#[cfg(feature = "actix")]
#[actix_web::test]
async fn actix_extension_survives_the_service_pipeline() {
    use actix_web::{App, test, web};

    async fn failing() -> Result<&'static str, treat::ApiError> {
        Err(error("boom").with_message("m").with_status(404).with_pointer("/email"))
    }

    let app = test::init_service(App::new().route("/", web::get().to(failing))).await;
    let response = test::call_service(&app, test::TestRequest::get().uri("/").to_request()).await;

    assert_eq!(response.status().as_u16(), 404);
    let handler = treat::response_get_api_error_actix(response.response()).expect("handler in extensions");
    assert_eq!(handler.code(), "boom");
    assert_eq!(handler.status(), 404);
}

/// An unset status must stay distinguishable from a deliberate one, otherwise a
/// logger cannot tell "the default applied" from "the handler chose this".
#[cfg(feature = "axum")]
#[test]
fn an_unset_status_reports_has_status_false() {
    use axum::response::IntoResponse;

    let response = error("boom").into_response();
    let handler = treat::response_get_api_error(&response).expect("handler in extensions");
    assert!(!handler.has_status());
    assert_eq!(handler.status(), treat::DEFAULT_ERROR_STATUS);
}

/// A typed code enum must erase to the same view as the default `&'static str`.
#[cfg(feature = "axum")]
#[test]
fn a_typed_code_erases_to_the_same_view() {
    use axum::response::IntoResponse;

    #[derive(Clone, Debug)]
    struct Code;
    impl std::fmt::Display for Code {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "typed.boom")
        }
    }

    let response = error(Code).with_status(404).into_response();
    let handler = treat::response_get_api_error(&response).expect("handler in extensions");
    assert_eq!(handler.code(), "typed.boom");
    assert_eq!(handler.status(), 404);
}
