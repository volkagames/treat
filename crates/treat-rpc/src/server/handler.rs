//! The steps of a generated handler: read the input, validate it, answer with
//! what the service returned.

use super::{QueryField, Refusal, http_status};
use crate::{HttpSpec, Runtime, RuntimeCodes, media};
use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde::de::{DeserializeOwned, IgnoredAny};
use treat_core::{
    ApiError,
    ApiErrorCode,
    ApiResponse,
    ErrorMessage,
    NoData,
    deserialize_body,
    error,
    failure,
    validation_error_messages,
};
use validator::{Validate, ValidationErrors};

/// A code of the runtime's own that displays as its wire name.
///
/// The code of an `ApiError` is written by `Display`, while the runtime's
/// other refusals write [`WireEnum::as_str`](crate::WireEnum::as_str). The
/// `ApiErrorCode` derive's `Display` reads the same names in the same order
/// (`#[code]`, serde's `rename`/`rename_all`, the identifier), so for a
/// derived registry the two agree; the wrapper is for a registry whose
/// `Display` is written by hand and says something else. Every `ApiError` the
/// runtime builds carries its code in it, so a refusal spells the code the
/// same way whichever step built it, and the way a client reads it back by
/// name.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WireCode<R>(pub(crate) R);

impl<R: crate::WireEnum> std::fmt::Display for WireCode<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl<R: RuntimeCodes> Runtime<R> {
    /// Reject an input that parsed but breaks a constraint the schema declares.
    ///
    /// The code is rewritten to [`RuntimeCodes::INVALID_BODY`] so every code on
    /// the wire stays a registry value instead of leaking `validator`'s own rule
    /// names; the rule that was broken survives in `message`, and the RFC 6901
    /// pointer says which field it was.
    pub fn invalid_input(violations: &ValidationErrors) -> Response {
        let mut messages = validation_error_messages(violations);
        for message in &mut messages {
            message.code = R::INVALID_BODY.as_str().to_string();
        }
        let response: ApiResponse<NoData> = failure(messages);
        response.into_response()
    }

    /// The envelope for a failure that is this server's own fault, with
    /// [`RuntimeCodes::INTERNAL_ERROR`].
    pub fn internal_failure(message: &str) -> Response {
        let response: ApiResponse<NoData> = failure(vec![ErrorMessage {
            code: R::INTERNAL_ERROR.as_str().to_string(),
            message: Some(message.to_string()),
            ..Default::default()
        }]);
        response.into_response()
    }

    /// Check an input against its `validator` rules, refusing it with
    /// [`Runtime::invalid_input`].
    ///
    /// Called on its own only where the input is put together in steps — a raw
    /// request's, whose body field is filled after the query string is read;
    /// the `*_input` helpers validate what they read.
    pub fn validate<T: Validate>(input: &T) -> Result<(), Refusal> {
        input
            .validate()
            .map_err(|violations| Self::invalid_input(&violations).into())
    }

    /// An RPC operation's input, read from its JSON body and validated.
    ///
    /// The request must declare a JSON body (see [`Runtime::no_input`] for
    /// why), else it is refused at `415` without being read. A body that cannot
    /// be read is refused at the status axum gave that — `413` over the
    /// router's body limit, `400` for one that broke off (the connection
    /// dropped) — and one that does not parse as `T` at `422`,
    /// with a pointer to where it went wrong; a rule the parsed input breaks is
    /// [`Runtime::invalid_input`] at the envelope's `200`. Every one of them
    /// carries the registry's [`RuntimeCodes::INVALID_BODY`].
    pub async fn json_input<T: DeserializeOwned + Validate>(request: Request) -> Result<T, Refusal> {
        Self::require_json(request.headers())?;
        let input: T = Self::read_json(request, StatusCode::UNPROCESSABLE_ENTITY).await?;
        Self::validate(&input)?;
        Ok(input)
    }

    /// The body of an RPC operation without an input, which must still be JSON.
    ///
    /// Read and discarded rather than ignored, so a caller sending something
    /// other than JSON — or nothing — hears that its request is malformed, the
    /// same as for an operation with an input, and refused the same way as
    /// [`Runtime::json_input`] refuses.
    ///
    /// The `Content-Type` is checked before anything else, here and in
    /// `json_input`: it must be `application/json` or a `+json` type, in any
    /// case and with any parameters. That is what keeps a page on another
    /// origin from calling an operation with its visitor's cookies. A browser
    /// sends a cross-origin request without asking the server first (a CORS
    /// preflight) only when it is a "simple" one — a `POST` among them — whose
    /// content type is a form's or plain text's (`application/x-www-form-urlencoded`,
    /// `multipart/form-data`, `text/plain`). A server that read its body under
    /// any type would run such a request before CORS could refuse to show the
    /// answer; one that insists on JSON makes the browser ask first.
    ///
    /// The other kinds of route are guarded as follows:
    ///
    /// - a `@raw(request:)` body is read only under a type the operation
    ///   declares ([`Runtime::require_content_type`]), so it is guarded as far
    ///   as that list leaves out the three types above, and ranges covering
    ///   them (`text/*`, `*/*`). A request without a `Content-Type`, which a
    ///   browser also sends without a preflight, is read only where the list
    ///   admits one of them anyway, so it reaches no operation a form could
    ///   not;
    /// - a `@raw(response:)` operation reads its input as JSON, here;
    /// - a `@subscription` is a `GET`, which a cross-origin page can send but
    ///   not read the answer of: the service must not change state on one;
    /// - an `@http` route checks nothing, by design: it keeps the contract its
    ///   existing callers have, often a form post. One a cross-origin page must
    ///   not reach needs a guard of its own — a `SameSite` session cookie, an
    ///   anti-forgery token, an `Origin` check.
    pub async fn no_input(request: Request) -> Result<(), Refusal> {
        Self::require_json(request.headers())?;
        let _: IgnoredAny = Self::read_json(request, StatusCode::UNPROCESSABLE_ENTITY).await?;
        Ok(())
    }

    /// Refuse a request whose `Content-Type` is not JSON's, at `415`.
    fn require_json(headers: &HeaderMap) -> Result<(), Refusal> {
        let content_type = headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok());
        if content_type.is_some_and(media::is_json) {
            return Ok(());
        }
        Err(Self::unsupported_media_type(
            "the request body must be JSON, declared by `Content-Type: application/json`".to_string(),
        ))
    }

    /// Refuse a request whose `Content-Type` is none of the media ranges
    /// `declared`, at `415` with [`RuntimeCodes::INVALID_BODY`] and
    /// `source.header` naming `content-type`.
    ///
    /// What a `@raw(request: [...])` operation checks before its body is read,
    /// for the reason [`Runtime::no_input`] gives for an RPC body: a raw body
    /// read under any type is one a cross-origin form can post with its
    /// visitor's cookies. `Runtime::raw_body` (feature `stream`) calls it for a
    /// buffered body; a streamed one's handler calls it first, with the
    /// `RawRequest`'s headers, before the service gets the body.
    ///
    /// Each entry of `declared` is a media range as the schema lists it:
    /// `text/csv` takes that type alone, `image/*` any subtype of `image`, and
    /// `*/*` any type. Types are compared by `type/subtype` alone, without
    /// regard to case and with parameters (`; charset=utf-8`) on either side
    /// ignored. A header that is present but not a readable value (not UTF-8)
    /// matches nothing.
    ///
    /// A request with no `Content-Type` at all — what `curl -T` sends — is
    /// accepted exactly where the list already admits a CORS-safelisted type
    /// (`application/x-www-form-urlencoded`, `multipart/form-data`,
    /// `text/plain`, or a range covering one: `*/*`, `text/*`,
    /// `multipart/*`), and refused everywhere else. A browser sends an untyped
    /// body (a `BufferSource`, a `Blob` without a type) cross-origin without a
    /// preflight too, so absence is a simple request like a form post: where
    /// the list admits a form post, the operation is already reachable from
    /// another origin and refusing absence protects nothing; where it does not
    /// (`application/octet-stream`, `text/csv`, `image/*`), refusing absence
    /// is what keeps the browser asking first. The runtime does not supply a
    /// type for such a request: the service sees the headers as sent, without
    /// one, and decides how to read the body itself.
    ///
    /// A list that admits a form post, whether by naming one of those types or
    /// by a range covering one, admits a cross-origin request: such an
    /// operation needs another guard, as an `@http` one does.
    pub fn require_content_type(headers: &HeaderMap, declared: &[&str]) -> Result<(), Refusal> {
        let accepted = match headers.get(header::CONTENT_TYPE) {
            None => admits_a_simple_request(declared),
            Some(value) => {
                value
                    .to_str()
                    .is_ok_and(|content_type| declared.iter().any(|range| media::matches_range(content_type, range)))
            }
        };
        if accepted {
            return Ok(());
        }
        let message = match declared {
            [] => "the operation accepts no request body".to_string(),
            declared => {
                format!(
                    "the request body must be declared by `Content-Type` as one of `{}`",
                    declared.join("`, `")
                )
            }
        };
        Err(Self::unsupported_media_type(message))
    }

    /// The `415` refusal of a body under a type the operation does not read.
    fn unsupported_media_type(message: String) -> Refusal {
        error(WireCode(R::INVALID_BODY))
            .with_message(message)
            .with_header(header::CONTENT_TYPE.as_str())
            .with_status(StatusCode::UNSUPPORTED_MEDIA_TYPE.as_u16())
            .into()
    }

    /// Read a JSON body as `T`, unvalidated.
    ///
    /// A body that cannot be read is refused at the status axum's rejection
    /// carries (`413` for one over the router's limit, `400` for one that
    /// broke off); one that does not
    /// parse, at `parse_status`, with the pointer `serde-path` found. Both with
    /// [`RuntimeCodes::INVALID_BODY`] rather than `treat`'s own `invalid_body`,
    /// so every code on the wire stays a registry value.
    pub(crate) async fn read_json<T: DeserializeOwned>(
        request: Request,
        parse_status: StatusCode,
    ) -> Result<T, Refusal> {
        let bytes = Self::read_body(request).await?;
        Self::parse_json(&bytes, parse_status)
    }

    /// The whole body of `request`; one that cannot be read is refused at the
    /// status axum's rejection carries (`413` for one over the router's limit,
    /// `400` for one that broke off while being read).
    pub(crate) async fn read_body(request: Request) -> Result<Bytes, Refusal> {
        let read = Bytes::from_request(request, &()).await;
        read.map_err(|rejection| {
            error(WireCode(R::INVALID_BODY))
                .with_message(rejection.body_text())
                .with_status(rejection.status().as_u16())
                .into()
        })
    }

    /// `bytes` parsed as `T`; JSON that does not parse is refused at
    /// `parse_status`, with the pointer `serde-path` found.
    pub(crate) fn parse_json<T: DeserializeOwned>(bytes: &[u8], parse_status: StatusCode) -> Result<T, Refusal> {
        deserialize_body::<T>(bytes).map_err(|parse_error| {
            parse_error
                .map_code(|_| WireCode(R::INVALID_BODY))
                .with_status(parse_status.as_u16())
                .into()
        })
    }

    /// An input read from the query string and validated: a subscription's, or
    /// a streamed raw request's.
    pub fn query_input<T: DeserializeOwned + Validate>(
        query: Option<&str>,
        fields: &[QueryField],
    ) -> Result<T, Refusal> {
        let input: T = Self::input_from_query(query, fields)?;
        Self::validate(&input)?;
        Ok(input)
    }

    /// Answer with an operation's enveloped result: the `ApiResponse` (or other
    /// response) it succeeded with, or the failure's envelope at the status the
    /// failure carries — `treat`'s default (`200`, since an RPC caller reads
    /// `errors[]`) unless it set one.
    pub fn respond<T: IntoResponse, C: ApiErrorCode>(result: Result<T, ApiError<C>>) -> Response {
        match result {
            Ok(response) => response.into_response(),
            Err(error) => error.into_response(),
        }
    }

    /// Answer with an `@http(envelope: false)` operation's result: the value
    /// itself as the JSON body, or the failure's envelope at `500` unless the
    /// failure set an error or redirect status of its own.
    ///
    /// A bare success is told from a failure by its status alone, so a failure
    /// cannot go out at a success status: one the service left at `200`, or
    /// gave another `2xx` or a `1xx`, answers `500` (see [`http_status`]). With feature
    /// `rpc-status-header` a success carries `x-rpc-status: ok`, as an
    /// envelope would.
    ///
    /// [`Runtime::respond_http`] picks between this and an envelope from the
    /// operation's [`HttpSpec`].
    pub fn respond_bare<T: Serialize, C: ApiErrorCode>(result: Result<T, ApiError<C>>) -> Response {
        match result {
            Ok(value) => {
                let response = axum::Json(value).into_response();
                // Not serializable: axum answers that `500` itself, which is
                // no success to mark.
                match response.status().is_success() {
                    true => mark_ok(response),
                    false => response,
                }
            }
            Err(error) => http_status(error.into_response(), StatusCode::INTERNAL_SERVER_ERROR),
        }
    }

    /// Answer with an `@http` operation's result, as `spec.envelope` says.
    ///
    /// - Enveloped, the success is the operation's `ApiResponse` (anything
    ///   that serializes as the envelope), written as `ApiResponse` writes
    ///   itself: its JSON at `200`, and with `rpc-status-header` the outcome its
    ///   `errors[]` reports. A failure is its envelope at the status it carries,
    ///   as [`Runtime::respond`] answers it.
    /// - Bare, as [`Runtime::respond_bare`]: the value itself, and a failure
    ///   at a `2xx` status answered `500`.
    ///
    /// Together with [`Runtime::http_input`] (`400`), [`Runtime::http_session`]
    /// (`401`) and [`Runtime::http_access`] (`403`), these are the statuses an
    /// `@http` caller reads.
    pub fn respond_http<T: Serialize, C: ApiErrorCode>(spec: &HttpSpec, result: Result<T, ApiError<C>>) -> Response {
        match result {
            Ok(envelope) if spec.envelope => enveloped(envelope),
            Err(error) if spec.envelope => error.into_response(),
            bare => Self::respond_bare(bare),
        }
    }
}

/// The request types a page on another origin can send with a body and
/// without a CORS preflight, besides none at all (the Fetch standard's
/// CORS-safelisted `Content-Type` values).
const CORS_SAFELISTED_TYPES: [&str; 3] = ["application/x-www-form-urlencoded", "multipart/form-data", "text/plain"];

/// Whether the media ranges `declared` admit a request a cross-origin page can
/// send without a preflight: one naming a CORS-safelisted type, or a range
/// covering one. Where they do, a request without a `Content-Type` — which a
/// browser also sends without a preflight — opens nothing new.
fn admits_a_simple_request(declared: &[&str]) -> bool {
    declared.iter().any(|range| {
        CORS_SAFELISTED_TYPES
            .iter()
            .any(|safelisted| media::matches_range(safelisted, range))
    })
}

/// A success that serializes as the envelope itself, written as `ApiResponse`'s
/// own `IntoResponse` writes one: its JSON, and with feature
/// `rpc-status-header` the `x-rpc-status` its `errors[]` reports — a service
/// may return an `ApiResponse` that carries errors.
fn enveloped<T: Serialize>(envelope: T) -> Response {
    #[cfg(feature = "rpc-status-header")]
    {
        let value = serde_json::to_value(&envelope);
        if let Ok(value) = value {
            let failed = value
                .get("errors")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|errors| !errors.is_empty());
            let outcome = match failed {
                true => treat_core::rpc_status::ERROR,
                false => treat_core::rpc_status::OK,
            };
            return mark(axum::Json(value).into_response(), outcome);
        }
    }
    // Not serializable: axum answers that `500` itself, unmarked.
    axum::Json(envelope).into_response()
}

/// A success that is not an envelope — a bare value, raw bytes, an event
/// stream — marked `x-rpc-status: ok` with feature `rpc-status-header`, so a
/// caller reading the header never finds it missing where `treat-core` would
/// have set it on an envelope. Without the feature, the response unchanged.
///
/// Set last, over any `x-rpc-status` already there: the header is the
/// runtime's account of the outcome, not something a service's own headers
/// can contradict.
pub(crate) fn mark_ok(response: Response) -> Response {
    #[cfg(feature = "rpc-status-header")]
    let response = mark(response, treat_core::rpc_status::OK);
    response
}

/// `response` with `x-rpc-status: outcome`, replacing any it had.
#[cfg(feature = "rpc-status-header")]
fn mark(mut response: Response, outcome: &'static str) -> Response {
    response.headers_mut().insert(
        axum::http::HeaderName::from_static(treat_core::rpc_status::X_RPC_STATUS),
        axum::http::HeaderValue::from_static(outcome),
    );
    response
}
