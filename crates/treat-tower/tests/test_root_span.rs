//! Regression (F7): the root span must record a real `http.host` / `http.scheme`.
//! Server-side HTTP/1.1 requests arrive in origin-form (`/path`), so the URI has
//! no authority/scheme; the resolvers must fall back to the `Host` and
//! `X-Forwarded-Proto` headers (as the actix middleware does) instead of the URI.

use axum::http::Request; // axum re-exports the same `http` crate the resolvers use
use treat_tower::root_span::{request_host, request_scheme};

#[test]
fn host_falls_back_to_host_header_on_origin_form() {
    let req = Request::builder()
        .uri("/users/1")
        .header("host", "api.example.com")
        .body(())
        .expect("request");
    assert_eq!(request_host(&req), "api.example.com");
}

#[test]
fn host_prefers_uri_authority_when_present() {
    // HTTP/2 populates the URI authority; it wins over the header.
    let req = Request::builder()
        .uri("https://uri.example.com/users")
        .header("host", "header.example.com")
        .body(())
        .expect("request");
    assert_eq!(request_host(&req), "uri.example.com");
}

#[test]
fn host_is_empty_without_uri_authority_or_header() {
    let req = Request::builder().uri("/path").body(()).expect("request");
    assert_eq!(request_host(&req), "");
}

#[test]
fn scheme_falls_back_to_forwarded_proto() {
    let req = Request::builder()
        .uri("/path")
        .header("x-forwarded-proto", "https")
        .body(())
        .expect("request");
    assert_eq!(request_scheme(&req), "https");
}

#[test]
fn scheme_prefers_uri_scheme_when_present() {
    let req = Request::builder()
        .uri("https://uri.example.com/users")
        .header("x-forwarded-proto", "http")
        .body(())
        .expect("request");
    assert_eq!(request_scheme(&req), "https");
}

#[test]
fn scheme_is_empty_without_uri_scheme_or_header() {
    let req = Request::builder().uri("/path").body(()).expect("request");
    assert_eq!(request_scheme(&req), "");
}
