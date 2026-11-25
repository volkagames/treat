//! Regression (F7): the root span must record a real `http.host` / `http.scheme`.
//! Server-side HTTP/1.1 requests arrive in origin-form (`/path`), so the URI has
//! no authority/scheme; the resolvers must fall back to the `Host` and
//! `X-Forwarded-Proto` headers (as the actix middleware does) instead of the URI.

use axum::http::Request; // axum re-exports the same `http` crate the resolvers use
use treat_tower::root_span::{request_client_ip, request_host, request_scheme};

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

/// Like actix's `connection_info`: a plain request with nothing to go on is
/// `http`, not an empty field.
#[test]
fn scheme_defaults_to_http_without_uri_scheme_or_header() {
    let req = Request::builder().uri("/path").body(()).expect("request");
    assert_eq!(request_scheme(&req), "http");
}

#[test]
fn client_ip_prefers_the_first_forwarded_for() {
    let req = Request::builder()
        .uri("/path")
        .header("forwarded", "proto=https; For=\"[2001:db8::1]:4711\", for=10.0.0.2")
        .header("x-forwarded-for", "10.0.0.3, 10.0.0.4")
        .body(())
        .expect("request");
    assert_eq!(request_client_ip(&req), "2001:db8::1");
}

#[test]
fn client_ip_falls_back_to_the_first_x_forwarded_for() {
    let req = Request::builder()
        .uri("/path")
        .header("x-forwarded-for", " 10.0.0.3 , 10.0.0.4")
        .body(())
        .expect("request");
    assert_eq!(request_client_ip(&req), "10.0.0.3");
}

#[test]
fn client_ip_falls_back_to_a_socket_addr_extension() {
    let mut req = Request::builder().uri("/path").body(()).expect("request");
    req.extensions_mut()
        .insert(std::net::SocketAddr::from(([192, 0, 2, 7], 50_000)));
    assert_eq!(request_client_ip(&req), "192.0.2.7");
}

#[test]
fn client_ip_is_empty_with_nothing_to_go_on() {
    let req = Request::builder().uri("/path").body(()).expect("request");
    assert_eq!(request_client_ip(&req), "");
}
