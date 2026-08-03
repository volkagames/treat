//! Turn a `serde_path_to_error` deserialization failure into an [`ApiError`]
//! that carries a JSON Pointer to the offending field.
//!
//! Enabled by the `serde-path` feature. Pairs with the field-level locators
//! (Phase 9 F1): the pointer lands in `ErrorMessage.source.pointer`.

use crate::{ApiError, error};

/// Error code used for body deserialization failures.
const INVALID_BODY: &str = "invalid_body";

/// HTTP status the request extractors report for a body that could not be parsed
/// or failed validation.
///
/// The envelope's own default is
/// [`DEFAULT_ERROR_STATUS`](crate::DEFAULT_ERROR_STATUS), but a rejected request
/// never produced a result, so the extractors answer `422 Unprocessable Content`
/// — the client sent syntactically valid JSON the server refused. Override it per
/// error with [`ApiError::with_status`](crate::ApiError::with_status).
pub const REJECTION_STATUS: u16 = 422;

/// Deserialize `bytes` as `T`, tracking the path to any failure.
///
/// On success returns `T`. On failure returns an [`ApiError`] with code
/// `"invalid_body"`, the serde message, and a JSON Pointer to the field via
/// [`with_pointer`](ApiError::with_pointer).
///
/// ```
/// # use serde::Deserialize;
/// #[derive(Debug, Deserialize)]
/// struct Body { email: String }
/// let err = leto_core::deserialize_body::<Body>(br#"{"email": 42}"#).unwrap_err();
/// assert_eq!(*err.code(), "invalid_body");
/// assert_eq!(err.error_source().and_then(|s| s.pointer.as_deref()), Some("/email"));
/// ```
pub fn deserialize_body<T>(bytes: &[u8]) -> Result<T, ApiError>
where
    T: serde::de::DeserializeOwned,
{
    let mut de = serde_json::Deserializer::from_slice(bytes);
    match serde_path_to_error::deserialize::<_, T>(&mut de) {
        Ok(value) => Ok(value),
        Err(err) => Err(error_from_path(err)),
    }
}

/// Deserialize from an existing [`serde_json::Value`], tracking the path.
///
/// Same contract as [`deserialize_body`], for callers that already parsed the
/// raw JSON (e.g. a framework that hands over a `Value`).
pub fn deserialize_value<T>(value: serde_json::Value) -> Result<T, ApiError>
where
    T: serde::de::DeserializeOwned,
{
    match serde_path_to_error::deserialize::<_, T>(value) {
        Ok(value) => Ok(value),
        Err(err) => Err(error_from_path(err)),
    }
}

/// Build the [`ApiError`] from a `serde_path_to_error` error: `"invalid_body"`
/// code, the serde message, a [`REJECTION_STATUS`] transport status, and a JSON
/// Pointer locator (omitted when the path is empty, e.g. a top-level type
/// mismatch).
fn error_from_path<E: std::fmt::Display>(err: serde_path_to_error::Error<E>) -> ApiError {
    let pointer = json_pointer(err.path());
    let message = err.inner().to_string();
    let err = error(INVALID_BODY).with_message(message).with_status(REJECTION_STATUS);
    match pointer {
        Some(pointer) => err.with_pointer(pointer),
        None => err,
    }
}

/// Convert a `serde_path_to_error` [`Path`](serde_path_to_error::Path) into an
/// RFC 6901 JSON Pointer (`/data/attributes/email`). Returns `None` for an
/// empty path (no locatable field).
fn json_pointer(path: &serde_path_to_error::Path) -> Option<String> {
    use serde_path_to_error::Segment;

    let mut pointer = String::new();
    for segment in path.iter() {
        pointer.push('/');
        match segment {
            Segment::Seq { index } => pointer.push_str(&index.to_string()),
            Segment::Map { key } | Segment::Enum { variant: key } => pointer.push_str(&escape_token(key)),
            // A field serde could not name — keep the pointer well-formed.
            Segment::Unknown => pointer.push('?'),
        }
    }

    (!pointer.is_empty()).then_some(pointer)
}

/// Escape a reference token per RFC 6901: `~` → `~0`, `/` → `~1`.
fn escape_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}
