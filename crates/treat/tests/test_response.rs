//! Coverage for the `ApiResponse` envelope: constructors, builders, accessors,
//! `inner*` extractors, `From` conversions and serde round-trips.

use treat::{ApiError, ApiResponse, ErrorMessage, error, failure, success};

fn msg(code: &str) -> ErrorMessage {
    ErrorMessage {
        code: code.to_string(),
        message: None,
        meta: None,
        ..Default::default()
    }
}

#[test]
fn success_is_data_only() {
    let resp = success(42_i32);
    assert_eq!(resp.data, Some(42));
    assert_eq!(resp.meta, None);
    assert!(resp.errors.is_empty());
    assert_eq!(resp.ok(), Some(&42));
    assert!(resp.err().is_none());
}

#[test]
fn failure_is_errors_only() {
    let resp: ApiResponse<i32> = failure(msg("boom"));
    assert_eq!(resp.data, None);
    assert_eq!(resp.errors.len(), 1);
    assert!(resp.ok().is_none());
    assert_eq!(resp.err().map(|e| e.code.as_str()), Some("boom"));
}

#[test]
fn builders_set_each_field() {
    let resp = ApiResponse::<i32, String>::from(1)
        .with_data(2)
        .with_meta("meta".to_string())
        .with_errors([msg("a"), msg("b")]);

    assert_eq!(resp.data, Some(2));
    assert_eq!(resp.meta.as_deref(), Some("meta"));
    assert_eq!(resp.errors.len(), 2);
}

#[test]
fn error_code_lookups() {
    let resp: ApiResponse<i32> = failure([msg("first"), msg("second")]);
    assert_eq!(resp.first_error_code(), Some("first"));
    assert_eq!(resp.last_error_code(), Some("second"));
    assert!(resp.has_error_code("second").is_some());
    assert!(resp.has_error_code("missing").is_none());
}

#[test]
fn as_result_wraps_ok() {
    let resp = success(1_i32);
    assert!(resp.as_result().is_ok());
}

#[test]
fn from_data_and_from_error() {
    let resp: ApiResponse<i32> = 7.into();
    assert_eq!(resp.data, Some(7));

    let resp: ApiResponse = error("boom").into();
    assert_eq!(resp.first_error_code(), Some("boom"));

    let resp: ApiResponse = (&error("boom2")).into();
    assert_eq!(resp.first_error_code(), Some("boom2"));
}

#[test]
fn from_result() {
    let ok: Result<i32, ApiError> = Ok(3);
    let resp: ApiResponse<i32> = ok.into();
    assert_eq!(resp.data, Some(3));

    let err: Result<i32, ApiError> = Err(error("nope"));
    let resp: ApiResponse<i32> = err.into();
    assert_eq!(resp.first_error_code(), Some("nope"));
}

// A local error type usable with the `inner`/`into_inner` extractors.
#[derive(Debug)]
struct MyErr(String);
impl From<&ErrorMessage> for MyErr {
    fn from(m: &ErrorMessage) -> Self {
        MyErr(m.code.clone())
    }
}
impl From<ErrorMessage> for MyErr {
    fn from(m: ErrorMessage) -> Self {
        MyErr(m.code)
    }
}
impl From<MyErr> for erris::Report {
    fn from(e: MyErr) -> Self {
        erris::report!("{}", e.0)
    }
}

#[test]
fn inner_extractors() {
    let ok = success(9_i32);
    assert_eq!(ok.inner::<MyErr>().expect("ok").copied(), Some(9));
    assert_eq!(ok.inner_data().expect("ok"), &9);

    let bad: ApiResponse<i32> = failure(msg("bad"));
    let e = bad.inner::<MyErr>().expect_err("should be err");
    assert_eq!(e.0, "bad");
    assert!(bad.inner_data().is_err());
}

#[test]
fn into_inner_extractors() {
    let ok = success(9_i32);
    assert_eq!(ok.into_inner::<MyErr>().expect("ok"), Some(9));

    let bad: ApiResponse<i32> = failure(msg("bad"));
    assert!(bad.into_inner::<MyErr>().is_err());

    let bad: ApiResponse<i32> = failure(msg("bad"));
    assert!(bad.into_inner_data().is_err());
}

#[test]
fn serde_roundtrip_skips_empty_fields() {
    let resp = success(42_i32);
    let json = serde_json::to_value(&resp).expect("serialize");
    assert_eq!(json, serde_json::json!({ "data": 42 }));

    let back: ApiResponse<i32> = serde_json::from_value(json).expect("deserialize");
    assert_eq!(back.data, Some(42));

    let resp: ApiResponse<i32> = failure(msg("e"));
    let json = serde_json::to_value(&resp).expect("serialize");
    assert_eq!(json, serde_json::json!({ "errors": [{ "code": "e" }] }));
}

#[test]
fn success_always_carries_data_even_when_null() {
    // A success document (no errors) must always expose `data`, serializing an
    // absent payload as `"data": null` rather than dropping the field.
    let resp: ApiResponse<i32> = ApiResponse {
        data: None,
        meta: None,
        errors: vec![],
    };
    let json = serde_json::to_value(&resp).expect("serialize");
    assert_eq!(json, serde_json::json!({ "data": null }));
}

#[test]
fn error_document_omits_data_and_never_coexists() {
    // Even if a builder left `data` set, a non-empty `errors` makes this an error
    // document: `data` is dropped, and `data`/`errors` never coexist on the wire.
    let resp = ApiResponse::<i32, String> {
        data: Some(5),
        meta: Some("m".to_string()),
        errors: vec![msg("e")],
    };
    let json = serde_json::to_value(&resp).expect("serialize");
    assert_eq!(json, serde_json::json!({ "meta": "m", "errors": [{ "code": "e" }] }));
}

#[test]
fn empty_errors_are_never_serialized_as_array() {
    // `errors` is either absent or a non-empty array — never `"errors": []`.
    let resp = success(1_i32);
    let json = serde_json::to_value(&resp).expect("serialize");
    assert_eq!(json.get("errors"), None);
}
