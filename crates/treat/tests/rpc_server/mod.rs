//! What the `test_rpc_server_*` tests share: a registry and a way to read a
//! response.
#![allow(dead_code, reason = "each test crate uses its own part of this module")]

use axum::response::IntoResponse;
use treat::rpc::RuntimeCodes;
use treat::rpc::server::QueryField;

/// A registry whose runtime codes are spelled unlike `treat`'s own
/// `invalid_body`, so a test tells which of the two answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, treat::ApiErrorCode, treat::rpc::WireEnum)]
pub enum ErrorCode {
    #[code("rpc::invalid_body")]
    InvalidBody,
    #[code("rpc::internal_error")]
    InternalError,
    #[code("not_found")]
    NotFound,
    #[code("unauthenticated")]
    Unauthenticated,
}

impl RuntimeCodes for ErrorCode {
    const INVALID_BODY: Self = Self::InvalidBody;
    const INTERNAL_ERROR: Self = Self::InternalError;
}

pub type Rt = treat::rpc::Runtime<ErrorCode>;

/// The registry's `INVALID_BODY` on the wire.
pub const INVALID: &str = "rpc::invalid_body";

/// A generated table of query fields is a `static` like this one.
pub static FIND_FIELDS: &[QueryField] = &[
    QueryField::str("email"),
    QueryField::int("limit"),
    QueryField::str("tags").list(),
];

/// The response's status and its body as JSON (`null` for an empty body).
pub async fn read(response: impl IntoResponse) -> (u16, serde_json::Value) {
    let response = response.into_response();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body should buffer");
    if bytes.is_empty() {
        return (status, serde_json::Value::Null);
    }
    let json = serde_json::from_slice(&bytes).expect("the body should be JSON");
    (status, json)
}

/// The first error's code in an envelope.
pub fn code(json: &serde_json::Value) -> Option<&str> {
    json.pointer("/errors/0/code").and_then(serde_json::Value::as_str)
}
