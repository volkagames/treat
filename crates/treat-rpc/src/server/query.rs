//! Inputs read from the URL: the query string, a form body, a path.
//!
//! Where an operation has no JSON body to read its input from — a subscription
//! opened by `EventSource`, which can only GET; a raw request, whose one body is
//! the payload; an `@http` operation bound to a query string or a form — the
//! input object is rebuilt from `key=value` text, with each value's JSON type
//! taken from the schema.

use super::Refusal;
use crate::{Runtime, RuntimeCodes};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
/// The field table is `treat-core`'s, shared with the client so both ends
/// spell a field the same way; re-exported here, where a server names it.
pub use treat_core::rpc::{QueryField, QueryKind};
use treat_core::{ApiResponse, ErrorMessage, ErrorSource, NoData, failure};

impl<R: RuntimeCodes> Runtime<R> {
    /// Rebuild an operation's input from the query string.
    ///
    /// Used where there is no JSON body to read it from: a subscription is
    /// opened by `EventSource`, which can only GET, and a raw request has
    /// already spent its one body on the payload.
    ///
    /// An absent parameter is left out of the object rather than sent as null,
    /// so an optional field takes its `#[serde(default)]` and a missing required
    /// one is reported by serde as the missing field it is.
    ///
    /// Not validated; [`Runtime::query_input`] is this plus the input's
    /// `validator` rules.
    pub fn input_from_query<T: DeserializeOwned>(query: Option<&str>, fields: &[QueryField]) -> Result<T, Refusal> {
        let pairs = Self::url_pairs(query.unwrap_or_default().as_bytes())?;
        Self::input_from_pairs(&pairs, fields)
    }

    /// The `key=value` pairs of a query string or a form body, decoded.
    ///
    /// Bytes, not text, and refused rather than altered when a pair is not
    /// UTF-8 once decoded — escaped (`%E9`) or raw, as a legacy Latin-1 form may
    /// send it. Replacing the byte would hand the service a value nobody sent,
    /// a password among them, and make two different ones equal. A refused
    /// value names its key in `source.parameter`; a refused key has no name
    /// to give.
    pub fn url_pairs(encoded: &[u8]) -> Result<Vec<(String, String)>, Refusal> {
        let mut pairs = Vec::new();
        for pair in encoded.split(|byte| *byte == b'&') {
            if pair.is_empty() {
                continue;
            }
            let mut halves = pair.splitn(2, |byte| *byte == b'=');
            let key = Self::url_text(halves.next().unwrap_or_default())?;
            let value = String::from_utf8(url_bytes(halves.next().unwrap_or_default()))
                .map_err(|_| Self::not_utf8(Some(&key)))?;
            pairs.push((key, value));
        }
        Ok(pairs)
    }

    /// One `application/x-www-form-urlencoded` component as text: `+` is a
    /// space, `%XX` the byte it names (in either case), and a `%` without two
    /// hex digits after it itself. One that is not UTF-8 once decoded is
    /// refused without naming a parameter; [`Runtime::url_pairs`] names the
    /// key of a value it refuses.
    pub fn url_text(encoded: &[u8]) -> Result<String, Refusal> {
        String::from_utf8(url_bytes(encoded)).map_err(|_| Self::not_utf8(None))
    }

    /// The refusal of a query or form component that is not UTF-8 text once
    /// decoded, naming `parameter` when there is one to name.
    fn not_utf8(parameter: Option<&str>) -> Refusal {
        Self::invalid_query("a query or form parameter is not UTF-8 text", parameter).into()
    }

    /// Rebuild an operation's input from decoded `key=value` pairs, each
    /// converted to the JSON type its field declares.
    ///
    /// A pair no field names is ignored; a non-list field repeated takes its
    /// first value.
    pub fn input_from_pairs<T: DeserializeOwned>(
        pairs: &[(String, String)],
        fields: &[QueryField],
    ) -> Result<T, Refusal> {
        let object = Self::object_from_pairs(pairs, fields)?;
        Self::input_from_object(object)
    }

    /// The input object [`Runtime::input_from_pairs`] reads, before serde does:
    /// a field per entry of `fields` the pairs carry, typed as it declares.
    pub(crate) fn object_from_pairs(
        pairs: &[(String, String)],
        fields: &[QueryField],
    ) -> Result<serde_json::Map<String, serde_json::Value>, Refusal> {
        let mut object = serde_json::Map::new();
        for field in fields {
            let mut values = pairs
                .iter()
                .filter(|(key, _)| key == field.name)
                .map(|(_, value)| value.as_str());

            if field.list {
                let items = values
                    .map(|value| Self::query_value(field, value))
                    .collect::<Result<Vec<_>, _>>()?;
                // An empty list and an absent one are the same thing in a query
                // string, so the field is left out and takes its default.
                if !items.is_empty() {
                    object.insert(field.name.to_string(), serde_json::Value::Array(items));
                }
                continue;
            }
            let first = values.next();
            if let Some(value) = first {
                let value = Self::query_value(field, value)?;
                object.insert(field.name.to_string(), value);
            }
        }
        Ok(object)
    }

    /// The input an object rebuilt from URL text describes; one serde refuses
    /// is [`Runtime::invalid_query`], with serde's reason as the message.
    ///
    /// The refusal names the parameter whose value serde refused in
    /// `source.parameter`, as [`Runtime::query_value`] does for one whose
    /// text is not of its kind: a value can be the right kind and still not
    /// the field's type — `id=not-a-uuid` is a string, but no `Uuid`;
    /// `limit=300` an integer, but no `u8`. The parameter is the top-level
    /// key the failure is under, also when it lies deeper inside a `Json`
    /// field's value or a list's item, since a parameter is what the caller
    /// wrote. A failure of the object as a whole — a required field missing —
    /// names none.
    pub(crate) fn input_from_object<T: DeserializeOwned>(
        object: serde_json::Map<String, serde_json::Value>,
    ) -> Result<T, Refusal> {
        let read = serde_path_to_error::deserialize(serde_json::Value::Object(object));
        read.map_err(|error| {
            let outermost = error.path().iter().next();
            let parameter = match outermost {
                Some(serde_path_to_error::Segment::Map { key }) => Some(key.as_str()),
                _ => None,
            };
            Self::invalid_query(&error.inner().to_string(), parameter).into()
        })
    }

    /// One parameter as the JSON value its declared type calls for; a refusal
    /// names the parameter in `source.parameter`.
    pub fn query_value(field: &QueryField, raw: &str) -> Result<serde_json::Value, Refusal> {
        let parsed = match field.kind {
            QueryKind::Str => Some(serde_json::Value::String(raw.to_string())),
            QueryKind::Int => raw.parse::<i64>().ok().map(Into::into),
            QueryKind::Float => {
                raw.parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map(serde_json::Value::Number)
            }
            QueryKind::Bool => raw.parse::<bool>().ok().map(serde_json::Value::Bool),
            QueryKind::Json => serde_json::from_str(raw).ok(),
        };
        parsed.ok_or_else(|| {
            Self::invalid_query(
                &format!("`{raw}` is not a valid {}", field.kind.describe()),
                Some(field.name),
            )
            .into()
        })
    }

    /// Reject a query string that does not describe the operation's input.
    ///
    /// The same code as a malformed body, [`RuntimeCodes::INVALID_BODY`] — it is
    /// the same failure, reached through the URL because that is where these
    /// operations carry their input — with the offending parameter named in
    /// `source` the way a pointer names a field. Answers `200`, as every
    /// envelope does; an `@http` handler gives it its status with
    /// [`http_status`](super::http_status).
    pub fn invalid_query(message: &str, parameter: Option<&str>) -> Response {
        let mut error = ErrorMessage {
            code: R::INVALID_BODY.as_str().to_string(),
            message: Some(message.to_string()),
            ..Default::default()
        };
        if let Some(parameter) = parameter {
            error.source = Some(ErrorSource::default().with_parameter(parameter));
        }
        let response: ApiResponse<NoData> = failure(vec![error]);
        response.into_response()
    }
}

/// One `application/x-www-form-urlencoded` component decoded to its bytes:
/// `+` is a space, `%XX` the byte it names (in either case), and a `%` without
/// two hex digits after it itself.
fn url_bytes(encoded: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut rest = encoded;
    while let [first, tail @ ..] = rest {
        rest = tail;
        let escaped = match (*first, tail) {
            (b'%', [high, low, after @ ..]) if high.is_ascii_hexdigit() && low.is_ascii_hexdigit() => {
                let digits = [*high, *low];
                let byte = std::str::from_utf8(&digits)
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok());
                byte.map(|byte| (byte, after))
            }
            _ => None,
        };
        match (escaped, *first) {
            (Some((byte, after)), _) => {
                bytes.push(byte);
                rest = after;
            }
            (None, b'+') => bytes.push(b' '),
            (None, other) => bytes.push(other),
        }
    }
    bytes
}
