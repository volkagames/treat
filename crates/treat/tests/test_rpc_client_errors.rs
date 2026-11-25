//! `treat::rpc::client`: reading a failed call — `ApiErrors` against the flat
//! registry and the operation's set — and the client's public surface.
//!
//! What goes over the wire is covered end to end by `test_rpc_e2e`, and the
//! private helpers behind it by `treat-rpc`'s unit tests.
#![cfg(all(feature = "rpc-client", feature = "derive"))]
#![allow(non_camel_case_types)]

use serde::{Deserialize, Serialize};
use treat::prelude::*;
use treat::rpc::client::{
    ApiErrors,
    ApiResponse,
    ClientError,
    ClientResult,
    ErrorMessage,
    ErrorSource,
    NoInput,
    RpcClient,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ApiErrorCode, treat::rpc::WireEnum)]
enum ErrorCode {
    #[message("invalid body")]
    invalid_body,
    #[message("internal error")]
    internal_error,
    #[message("not found")]
    not_found,
    #[message("gone")]
    #[code("things::gone")]
    #[serde(rename = "things::gone")]
    things__gone,
}

/// The set one operation declares, sharing wire names with the registry.
#[derive(Debug, Clone, Copy, PartialEq, ApiErrorCode, treat::rpc::WireEnum)]
enum GetCodes {
    #[code("not_found")]
    NotFound,
    #[code("things::gone")]
    ThingsGone,
    #[code("internal_error")]
    #[undeclared]
    InternalError,
}

fn message(code: &str) -> ErrorMessage {
    ErrorMessage {
        code: code.to_string(),
        ..ErrorMessage::default()
    }
}

fn failure(messages: Vec<ErrorMessage>) -> ApiErrors<GetCodes> {
    ApiErrors::new(messages)
}

#[test]
fn first_is_the_entry_the_server_listed_first() {
    let errors = failure(vec![message("not_found"), message("things::gone")]);
    assert_eq!(errors.first().map(|e| e.code.as_str()), Some("not_found"));
    assert_eq!(failure(vec![]).first(), None);
}

#[test]
fn pointer_is_the_first_entry_that_located_a_field() {
    let mut located = message("invalid_body");
    located.source = Some(ErrorSource::default().with_pointer("/email"));
    let mut parameter = message("invalid_body");
    parameter.source = Some(ErrorSource::default().with_parameter("page"));

    let errors = failure(vec![message("not_found"), parameter, located]);
    assert_eq!(errors.pointer(), Some("/email"));
    assert_eq!(failure(vec![message("not_found")]).pointer(), None);
}

#[test]
fn code_reads_the_first_entry_by_wire_name() {
    let errors = failure(vec![message("things::gone"), message("not_found")]);
    assert_eq!(errors.code::<ErrorCode>(), Some(ErrorCode::things__gone));
    assert_eq!(errors.code::<GetCodes>(), Some(GetCodes::ThingsGone));

    // Inferred from the pattern, as a caller's `match` would.
    let label = match errors.code() {
        Some(ErrorCode::things__gone) => "gone",
        Some(_) => "other",
        None => "unknown",
    };
    assert_eq!(label, "gone");
}

#[test]
fn code_is_none_for_a_code_the_client_does_not_know() {
    // A server newer than the client.
    let errors = failure(vec![message("brand_new")]);
    assert_eq!(errors.code::<ErrorCode>(), None);
    // The registry knows it, the operation's set does not.
    let errors = failure(vec![message("invalid_body")]);
    assert_eq!(errors.code::<ErrorCode>(), Some(ErrorCode::invalid_body));
    assert_eq!(errors.code::<GetCodes>(), None);
    // The rust identifier is not a wire name.
    assert_eq!(failure(vec![message("things__gone")]).code::<ErrorCode>(), None);
    assert_eq!(failure(vec![]).code::<ErrorCode>(), None);
}

#[test]
fn is_and_meta_for_look_at_every_entry() {
    let mut gone = message("things::gone");
    gone.meta = serde_json::json!({"since": "2026-01-01"}).as_object().cloned();
    let mut second = message("things::gone");
    second.meta = serde_json::json!({"since": "later"}).as_object().cloned();

    let errors = failure(vec![message("not_found"), gone, second]);
    assert!(errors.is(GetCodes::NotFound));
    assert!(errors.is(GetCodes::ThingsGone));
    assert!(!errors.is(GetCodes::InternalError));

    let meta = errors.meta_for(GetCodes::ThingsGone);
    assert_eq!(
        meta.and_then(|m| m.get("since")),
        Some(&serde_json::json!("2026-01-01"))
    );
    assert_eq!(errors.meta_for(GetCodes::NotFound), None);
    assert_eq!(errors.meta_for(GetCodes::InternalError), None);
}

/// A set named through serde alone, whose `Display` would be the identifier:
/// `is` and `meta_for` read the wire name.
#[derive(Debug, Clone, Copy, Serialize, treat::rpc::WireEnum)]
enum Renamed {
    #[serde(rename = "things::gone")]
    Gone,
}

#[test]
fn is_and_meta_for_compare_wire_names() {
    let mut gone = message("things::gone");
    gone.meta = serde_json::json!({"k": 1}).as_object().cloned();
    let errors: ApiErrors<Renamed> = ApiErrors::new(vec![gone]);
    assert!(errors.is(Renamed::Gone));
    assert!(errors.meta_for(Renamed::Gone).is_some());

    let errors: ApiErrors<Renamed> = ApiErrors::new(vec![message("Gone")]);
    assert!(!errors.is(Renamed::Gone), "the identifier is not the wire name");
}

#[test]
fn display_shows_the_first_entry() {
    let mut described = message("not_found");
    described.message = Some("no such user".to_string());
    assert_eq!(
        failure(vec![described, message("x")]).to_string(),
        "not_found: no such user"
    );
    assert_eq!(failure(vec![message("things::gone")]).to_string(), "things::gone");
    assert_eq!(failure(vec![]).to_string(), "request failed without an error code");
}

#[test]
fn client_error_display_names_where_the_call_failed() {
    let cases: Vec<(ClientError<GetCodes>, &str)> = vec![
        (ClientError::Api(failure(vec![message("not_found")])), "not_found"),
        (ClientError::Transport("reset".into()), "transport error: reset"),
        (
            ClientError::Encode("path parameter `id` is missing".into()),
            "could not encode request: path parameter `id` is missing",
        ),
        (ClientError::Decode("eof".into()), "could not decode response: eof"),
        (ClientError::Status(502), "request failed with status 502"),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let boxed: Box<dyn std::error::Error> = Box::new(error);
        assert_eq!(boxed.to_string(), expected);
    }
}

#[test]
fn client_result_is_a_result_over_client_error() {
    let result: ClientResult<u32, GetCodes> = Err(ClientError::Status(404));
    assert!(matches!(result, Err(ClientError::Status(404))));
}

#[test]
fn no_input_is_an_empty_object() {
    assert_eq!(serde_json::to_string(&NoInput).ok().as_deref(), Some("{}"));
}

#[test]
fn base_url_drops_trailing_slashes() {
    let client = RpcClient::new("http://localhost:8080/api//");
    assert_eq!(client.base_url(), "http://localhost:8080/api");
    let client = RpcClient::with_http("http://localhost:8080", treat::rpc::client::reqwest::Client::new());
    assert_eq!(client.clone().base_url(), "http://localhost:8080");
}

#[test]
fn api_response_reads_meta_as_a_map() {
    let response: ApiResponse<u32> = serde_json::from_str(r#"{"data":1,"meta":{"page":2}}"#).expect("envelope");
    assert_eq!(response.data, Some(1));
    assert_eq!(
        response.meta.and_then(|m| m.get("page").cloned()),
        Some(serde_json::json!(2))
    );
}

#[test]
fn call_futures_are_send() {
    // A generated method is an `async fn` over these; a caller spawns it.
    fn assert_send<T: Send>(_: T) {}
    let client = RpcClient::new("http://localhost");
    assert_send(client.call::<_, u32, GetCodes>("/x", &NoInput));
    static BY_ID: treat::rpc::HttpSpec = treat::rpc::HttpSpec {
        method: treat::rpc::HttpMethod::Get,
        path: "/x/{id}",
        fields: &[],
        path_fields: &["id"],
        body: treat::rpc::HttpBodyKind::None,
        envelope: false,
    };
    assert_send(client.call_http::<_, u32, GetCodes>(&BY_ID, &serde_json::json!({"id": 1})));
}
