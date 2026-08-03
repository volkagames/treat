//! Coverage for `ErrorMessage`: serde, conversions to `Value`/`Report`,
//! `IntoIterator`, `Display` and `into_response`.

use leto::{ApiResponse, ErrorMessage};
use serde_json::json;

fn full() -> ErrorMessage {
    ErrorMessage {
        code: "c".to_string(),
        message: Some("m".to_string()),
        meta: Some(json!({ "k": 1 })),
        ..Default::default()
    }
}

#[test]
fn serde_skips_none_fields() {
    let bare = ErrorMessage {
        code: "c".to_string(),
        message: None,
        meta: None,
        ..Default::default()
    };
    assert_eq!(serde_json::to_value(&bare).expect("serialize"), json!({ "code": "c" }));
    assert_eq!(
        serde_json::to_value(full()).expect("serialize"),
        json!({ "code": "c", "message": "m", "meta": { "k": 1 } })
    );

    let back: ErrorMessage = serde_json::from_value(json!({ "code": "x" })).expect("deserialize");
    assert_eq!(back.code, "x");
    assert!(back.message.is_none());
    assert!(back.meta.is_none());
}

#[test]
fn into_json_value() {
    let value: serde_json::Value = full().into();
    assert_eq!(value, json!({ "code": "c", "message": "m", "meta": { "k": 1 } }));

    let bare: serde_json::Value = ErrorMessage {
        code: "c".to_string(),
        message: None,
        meta: None,
        ..Default::default()
    }
    .into();
    assert_eq!(bare, json!({ "code": "c" }));
}

#[test]
fn into_report_preserves_display() {
    let report: erris::Report = full().into();
    assert!(report.to_string().contains('c'));
}

#[test]
fn into_iterator_yields_itself_once() {
    let items: Vec<ErrorMessage> = full().into_iter().collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].code, "c");
}

#[test]
fn display_renders_code_and_message() {
    assert_eq!(full().to_string(), "leto error: c, message: m");
    let bare = ErrorMessage {
        code: "c".to_string(),
        message: None,
        meta: None,
        ..Default::default()
    };
    assert_eq!(bare.to_string(), "leto error: c");
}

#[test]
fn into_response_builds_failure() {
    let resp: ApiResponse<i32> = full().into_response();
    assert_eq!(resp.first_error_code(), Some("c"));
    assert!(resp.ok().is_none());
}

#[test]
fn to_error_message_clones() {
    let m = full();
    assert_eq!(m.to_error_message(), m);
}
