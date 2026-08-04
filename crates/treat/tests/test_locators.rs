//! Coverage for error locators (JSON:API `source`) and RFC 9457 `type`/`instance`,
//! plus the OpenAPI schema of the envelope (feature `openapi`).

use treat::{ErrorSource, error};
use serde_json::json;

#[test]
fn builders_set_locator_fields() {
    let err = error("invalid")
        .with_pointer("/data/attributes/email")
        .with_parameter("page")
        .with_header("Authorization")
        .with_type("https://errors.example/invalid")
        .with_instance("req-42");

    let src = err.error_source().expect("source set");
    assert_eq!(src.pointer.as_deref(), Some("/data/attributes/email"));
    assert_eq!(src.parameter.as_deref(), Some("page"));
    assert_eq!(src.header.as_deref(), Some("Authorization"));
    assert_eq!(err.type_uri(), Some("https://errors.example/invalid"));
    assert_eq!(err.instance(), Some("req-42"));
}

#[test]
fn locators_serialize_into_error_message() {
    let msg = error("invalid")
        .with_pointer("/data/attributes/email")
        .with_type("https://errors.example/invalid")
        .with_instance("req-42")
        .to_error_message();

    assert_eq!(
        serde_json::to_value(&msg).expect("serialize"),
        json!({
            "code": "invalid",
            "type": "https://errors.example/invalid",
            "instance": "req-42",
            "source": { "pointer": "/data/attributes/email" },
        })
    );
}

#[test]
fn empty_source_is_omitted() {
    let value = serde_json::to_value(error("bare").to_error_message()).expect("serialize");
    assert_eq!(value, json!({ "code": "bare" }));
    assert!(value.get("source").is_none());
}

#[test]
fn value_conversion_matches_serde() {
    let msg = error("invalid")
        .with_parameter("page")
        .with_instance("req-1")
        .to_error_message();
    let via_serde = serde_json::to_value(&msg).expect("serialize");
    let via_into: serde_json::Value = msg.into();
    assert_eq!(via_serde, via_into);
}

#[test]
fn error_source_standalone_builder() {
    let src = ErrorSource::default()
        .with_pointer("/a")
        .with_parameter("b")
        .with_header("c");
    assert!(!src.is_empty());
    assert!(ErrorSource::default().is_empty());
}

#[cfg(feature = "openapi")]
#[test]
fn openapi_schemas_generate_for_envelope() {
    use utoipa::PartialSchema;

    // The envelope and its error object must produce a schema (F2).
    let _error_response = treat::ErrorResponse::schema();
    let _error_message = treat::ErrorMessage::schema();
    let _source = treat::ErrorSource::schema();
    let _envelope = treat::ApiResponse::<String, String>::schema();
}
