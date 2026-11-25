//! RPC calls, and the one reader of the envelope every answer goes through.

use super::{ApiErrors, ClientError, ClientResult, ErrorMessage, RpcClient};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

impl RpcClient {
    /// Issue one RPC call — `POST` with the input as a JSON body — and unwrap
    /// the envelope.
    ///
    /// The body goes with `Content-Type: application/json`, which the server
    /// requires of an RPC body (it refuses anything else at `415`).
    ///
    /// The status line is not the signal: an RPC route answers a failure with
    /// `200 OK` unless the service gave it another status, so `errors[]`
    /// decides the outcome and is checked first.
    pub async fn call<I, O, C>(&self, path: &str, input: &I) -> ClientResult<O, C>
    where
        I: Serialize + ?Sized,
        O: DeserializeOwned,
    {
        let request = json_body(self.http.post(self.url(path)), input)?;
        let response = send(request).await?;
        receive(response, true).await
    }
}

/// The envelope as the client reads it: the two fields that decide an answer.
///
/// Not `treat::ApiResponse`, whose `data` must be `Serialize` (and, with
/// `treat`'s `openapi` feature, a schema) — bounds a decoded value has no use
/// for. Envelope-level `meta` is not read: no call returns it.
///
/// `data` is `None` only when the key is absent. A present `"data": null` is
/// handed to `O` itself — treat writes every success with `data`, so that is
/// how an operation answering `()`, `None` or a null value travels, and it is
/// `O`'s to accept or refuse.
#[derive(Deserialize)]
#[serde(bound(deserialize = "O: Deserialize<'de>"))]
struct Envelope<O> {
    #[serde(default, deserialize_with = "present")]
    data: Option<O>,
    #[serde(default)]
    errors: Vec<ErrorMessage>,
}

/// A field that is present, whatever its value — `null` included, which
/// `Option`'s own `Deserialize` would read as absent.
fn present<'de, D, O>(deserializer: D) -> Result<Option<O>, D::Error>
where
    D: serde::Deserializer<'de>,
    O: Deserialize<'de>,
{
    O::deserialize(deserializer).map(Some)
}

/// The value or the failure an envelope carries.
///
/// `errors[]` wins whatever the status, then `data` (`null` included). A body
/// that is not an envelope, or one carrying neither key, is a [`ClientError::Decode`] under a 2xx
/// status and a [`ClientError::Status`] under any other: what answered was not
/// the service, or not one speaking this protocol.
pub(crate) fn read_envelope<O: DeserializeOwned, C>(status: StatusCode, body: &[u8]) -> ClientResult<O, C> {
    let envelope = serde_json::from_slice::<Envelope<O>>(body);
    let envelope = match envelope {
        Ok(envelope) => envelope,
        Err(e) => return Err(unusable(status, e.to_string())),
    };
    if !envelope.errors.is_empty() {
        return Err(ClientError::Api(ApiErrors::new(envelope.errors)));
    }
    match envelope.data {
        Some(data) => Ok(data),
        None => {
            Err(unusable(
                status,
                "response carried neither `data` nor `errors`".to_string(),
            ))
        }
    }
}

/// A successful answer as the operation sends it: the envelope, or with
/// `envelope` false the bare value. A non-2xx answer is the envelope either
/// way, since that is how every failure travels.
pub(crate) fn read_answer<O: DeserializeOwned, C>(
    status: StatusCode,
    body: &[u8],
    envelope: bool,
) -> ClientResult<O, C> {
    if envelope || !status.is_success() {
        return read_envelope(status, body);
    }
    serde_json::from_slice(body).map_err(|e| ClientError::Decode(e.to_string()))
}

/// The failure carried by an answer that is the envelope where the operation
/// sends something else — bytes or an event stream — on success.
///
/// Read like any envelope; one that carries `data` instead of `errors` is no
/// answer this operation gives, so it is a decode failure under a 2xx status.
#[cfg(feature = "stream")]
pub(crate) fn read_refusal<C>(status: StatusCode, body: &[u8]) -> ClientError<C> {
    let read = read_envelope::<serde::de::IgnoredAny, C>(status, body);
    match read {
        Err(error) => error,
        Ok(_) => {
            unusable(
                status,
                "expected the operation's own body, got an envelope carrying `data`".to_string(),
            )
        }
    }
}

/// The failure a body carries when it is an envelope with `errors[]`, and
/// `None` for any other body — an envelope carrying `data`, or no envelope.
///
/// For an answer that may be either the operation's own JSON or a refusal,
/// which nothing but the body tells apart.
#[cfg(feature = "stream")]
pub(crate) fn envelope_failure<C>(body: &[u8]) -> Option<ApiErrors<C>> {
    let envelope = serde_json::from_slice::<Envelope<serde::de::IgnoredAny>>(body).ok()?;
    (!envelope.errors.is_empty()).then(|| ApiErrors::new(envelope.errors))
}

/// A body with no usable envelope: [`ClientError::Decode`] if the status says
/// it succeeded, else [`ClientError::Status`].
fn unusable<C>(status: StatusCode, reason: String) -> ClientError<C> {
    if status.is_success() {
        ClientError::Decode(reason)
    } else {
        ClientError::Status(status.as_u16())
    }
}

/// `request` with `input` as its JSON body, declared `application/json` — the
/// one content type the server reads an RPC body under.
///
/// Serialized here rather than by `reqwest`, which would report a value serde
/// refuses as a transport error once the request is sent.
pub(crate) fn json_body<I: Serialize + ?Sized, C>(
    request: reqwest::RequestBuilder,
    input: &I,
) -> Result<reqwest::RequestBuilder, ClientError<C>> {
    let body = serde_json::to_vec(input).map_err(|e| ClientError::Encode(e.to_string()))?;
    Ok(request
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body))
}

/// Send `request`; a failure to get a response is [`ClientError::Transport`].
///
/// Except one `reqwest` reports while building the request — a URL that does
/// not parse, a header value that is not one — which is
/// [`ClientError::Encode`]: nothing was sent.
pub(crate) async fn send<C>(request: reqwest::RequestBuilder) -> Result<reqwest::Response, ClientError<C>> {
    request.send().await.map_err(|e| {
        match e.is_builder() {
            true => ClientError::Encode(e.to_string()),
            false => transport(e),
        }
    })
}

/// Read the whole of `response` as [`read_answer`] does.
pub(crate) async fn receive<O: DeserializeOwned, C>(response: reqwest::Response, envelope: bool) -> ClientResult<O, C> {
    let status = response.status();
    let body = response.bytes().await.map_err(transport)?;
    read_answer(status, &body, envelope)
}

/// Read `response` as [`read_refusal`] does, up to
/// [`REFUSAL_LIMIT`](RpcClient::REFUSAL_LIMIT) bytes: a body going past it is
/// no refusal, and the rest of it is not read.
#[cfg(feature = "stream")]
pub(crate) async fn refusal<C>(response: reqwest::Response) -> ClientError<C> {
    refusal_within(response, RpcClient::REFUSAL_LIMIT).await
}

#[cfg(feature = "stream")]
async fn refusal_within<C>(mut response: reqwest::Response, limit: usize) -> ClientError<C> {
    let status = response.status();
    let mut body: Vec<u8> = Vec::new();
    let mut ended = false;
    while !ended && body.len() <= limit {
        let chunk = response.chunk().await;
        match chunk {
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            Ok(None) => ended = true,
            Err(e) => return transport(e),
        }
    }
    if !ended {
        return unusable(status, format!("expected a refusal, got a body of over {limit} bytes"));
    }
    read_refusal(status, &body)
}

pub(crate) fn transport<C>(error: reqwest::Error) -> ClientError<C> {
    ClientError::Transport(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Codes {}

    fn envelope(status: u16, body: &str) -> ClientResult<serde_json::Value, Codes> {
        read_envelope(status_code(status), body.as_bytes())
    }

    fn status_code(status: u16) -> StatusCode {
        StatusCode::from_u16(status).unwrap_or(StatusCode::IM_A_TEAPOT)
    }

    #[test]
    fn errors_win_over_data_and_status() {
        let body = r#"{"errors":[{"code":"not_found","message":"no such user"},{"code":"gone"}]}"#;
        for status in [200, 404, 500] {
            let result = envelope(status, body);
            let codes: Vec<&str> = match &result {
                Err(ClientError::Api(errors)) => errors.messages.iter().map(|e| e.code.as_str()).collect(),
                _ => Vec::new(),
            };
            assert_eq!(codes, ["not_found", "gone"], "{status}: {result:?}");
        }
    }

    #[test]
    fn data_is_the_value_whatever_the_status() {
        assert_eq!(
            envelope(200, r#"{"data":{"id":1}}"#).ok(),
            Some(serde_json::json!({"id": 1}))
        );
        assert_eq!(
            envelope(201, r#"{"data":2,"meta":{"page":1}}"#).ok(),
            Some(serde_json::json!(2))
        );
        assert_eq!(envelope(404, r#"{"data":3}"#).ok(), Some(serde_json::json!(3)));
    }

    #[test]
    fn neither_data_nor_errors_is_decode_on_2xx_and_status_otherwise() {
        for body in ["{}", r#"{"errors":[]}"#, r#"{"meta":{"page":1}}"#] {
            let ok = envelope(200, body);
            assert!(matches!(ok, Err(ClientError::Decode(_))), "{body}: {ok:?}");
            let failed = envelope(502, body);
            assert!(matches!(failed, Err(ClientError::Status(502))), "{body}: {failed:?}");
        }
    }

    #[test]
    fn non_json_is_decode_on_2xx_and_status_otherwise() {
        for body in ["<html>bad gateway</html>", "", "42", r#"{"errors":"nope"}"#] {
            let ok = envelope(200, body);
            assert!(matches!(ok, Err(ClientError::Decode(_))), "{body}: {ok:?}");
            let failed = envelope(503, body);
            assert!(matches!(failed, Err(ClientError::Status(503))), "{body}: {failed:?}");
        }
    }

    #[test]
    fn null_data_is_a_value_for_whatever_accepts_null() {
        for body in [r#"{"data":null}"#, r#"{"data":null,"errors":[]}"#] {
            let unit: ClientResult<(), Codes> = read_envelope(StatusCode::OK, body.as_bytes());
            assert!(matches!(unit, Ok(())), "{body}: {unit:?}");

            let option: ClientResult<Option<u32>, Codes> = read_envelope(StatusCode::OK, body.as_bytes());
            assert!(matches!(option, Ok(None)), "{body}: {option:?}");

            assert_eq!(envelope(200, body).ok(), Some(serde_json::Value::Null), "{body}");
        }
        let some: ClientResult<Option<u32>, Codes> = read_envelope(StatusCode::OK, br#"{"data":5}"#);
        assert!(matches!(some, Ok(Some(5))), "{some:?}");
    }

    #[test]
    fn null_data_where_a_value_is_required_is_decode() {
        let required: ClientResult<u32, Codes> = read_envelope(StatusCode::OK, br#"{"data":null}"#);
        assert!(matches!(required, Err(ClientError::Decode(_))), "{required:?}");
    }

    #[test]
    fn data_of_the_wrong_shape_is_decode_on_2xx() {
        let result: ClientResult<u32, Codes> = read_envelope(StatusCode::OK, br#"{"data":"text"}"#);
        assert!(matches!(result, Err(ClientError::Decode(_))), "{result:?}");
    }

    #[test]
    fn bare_answer_is_the_value_on_2xx_and_the_envelope_otherwise() {
        let bare: ClientResult<u32, Codes> = read_answer(StatusCode::OK, b"7", false);
        assert_eq!(bare.ok(), Some(7));

        // A bare success is not unwrapped from `data`.
        let wrapped: ClientResult<u32, Codes> = read_answer(StatusCode::OK, br#"{"data":7}"#, false);
        assert!(matches!(wrapped, Err(ClientError::Decode(_))), "{wrapped:?}");

        let failure: ClientResult<u32, Codes> =
            read_answer(StatusCode::NOT_FOUND, br#"{"errors":[{"code":"not_found"}]}"#, false);
        assert!(matches!(failure, Err(ClientError::Api(_))), "{failure:?}");

        let page: ClientResult<u32, Codes> = read_answer(StatusCode::NOT_FOUND, b"<html/>", false);
        assert!(matches!(page, Err(ClientError::Status(404))), "{page:?}");

        let enveloped: ClientResult<u32, Codes> = read_answer(StatusCode::OK, br#"{"data":7}"#, true);
        assert_eq!(enveloped.ok(), Some(7));
    }

    #[cfg(feature = "stream")]
    #[test]
    fn refusal_is_the_envelope_failure() {
        let api = read_refusal::<Codes>(StatusCode::OK, br#"{"errors":[{"code":"forbidden"}]}"#);
        assert!(matches!(api, ClientError::Api(_)), "{api:?}");

        let data = read_refusal::<Codes>(StatusCode::OK, br#"{"data":{}}"#);
        assert!(matches!(data, ClientError::Decode(_)), "{data:?}");

        let data = read_refusal::<Codes>(StatusCode::BAD_GATEWAY, br#"{"data":{}}"#);
        assert!(matches!(data, ClientError::Status(502)), "{data:?}");

        let page = read_refusal::<Codes>(StatusCode::UNAUTHORIZED, b"denied");
        assert!(matches!(page, ClientError::Status(401)), "{page:?}");
    }
}
