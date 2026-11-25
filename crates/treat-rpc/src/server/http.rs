//! `@http` operations: an endpoint bound to a method and a path of its own,
//! whose callers read the status line.

use super::handler::WireCode;
use super::{Ctx, QueryField, Refusal};
use crate::{HttpBodyKind, HttpSpec, Runtime, RuntimeCodes, media};
use axum::body::Bytes;
use axum::extract::{FromRequest, FromRequestParts, RawPathParams, Request};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use treat_core::deserialize_value;
use validator::Validate;

/// Give a failure the status an `@http` caller reads.
///
/// The envelope's own failures answer `200`, since an RPC caller reads
/// `errors[]`; an endpoint bound to HTTP is read by callers that look at the
/// status line, so one that could not read its input says so there: `400`, or
/// whatever the body's rejection carried (`413` for one too large). A missing
/// session answers `401` and a denied one `403`, and the service's own failure
/// of an operation answered bare `500`: there a `200` would read as success.
/// A status the failure already carries other than `200` is kept.
pub fn http_status(mut response: Response, status: StatusCode) -> Response {
    if response.status() == StatusCode::OK {
        *response.status_mut() = status;
    }
    response
}

/// Replace every pair whose key `newer` names with `newer`'s pairs for it.
///
/// Per key rather than per pair, so a list field repeated in the body replaces
/// the query string's list as a whole instead of being merged into it.
pub fn override_pairs(pairs: &mut Vec<(String, String)>, newer: Vec<(String, String)>) {
    pairs.retain(|(key, _)| !newer.iter().any(|(new_key, _)| new_key == key));
    pairs.extend(newer);
}

/// The media type of a form body.
const FORM: &str = "application/x-www-form-urlencoded";

/// Whether a request declares a form body: its `Content-Type`'s media type is
/// exactly [`FORM`], in any case and with any parameters.
fn declares_form(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| media::is(value, FORM))
}

impl<R: RuntimeCodes> Runtime<R> {
    /// Read an `@http` operation's input from its request, as `spec` says, and
    /// validate it.
    ///
    /// [`Runtime::read_http_input`] and the input's `validator` rules, with a
    /// refusal of either given its status by [`http_status`]: `400`, a JSON
    /// body that does not parse included, unless the refusal already carries
    /// one (`413` for a body too large). Every refusal carries the registry's
    /// [`RuntimeCodes::INVALID_BODY`].
    pub async fn http_input<T: DeserializeOwned + Validate>(request: Request, spec: &HttpSpec) -> Result<T, Refusal> {
        let bad_request = |refusal: Refusal| refusal.or_status(StatusCode::BAD_REQUEST);
        let input: T = Self::read_http_input(request, spec).await.map_err(bad_request)?;
        Self::validate(&input).map_err(bad_request)?;
        Ok(input)
    }

    /// Read an `@http` operation's input from its request as `spec` says,
    /// unvalidated, with a refusal at the status it was built with: the
    /// envelope's `200`, except for a body that could not be read at all,
    /// which keeps the status of axum's rejection (`413` for one over the
    /// router's body limit).
    ///
    /// The path parameters `spec.path_fields` names win over everything else:
    /// they are what the route matched, so a body or a query string naming the
    /// same field cannot point the operation somewhere else. Each is typed as
    /// its entry in `spec.fields` says, as a query parameter would be; one the
    /// table does not list is read as a string, whatever the body kind. A path
    /// parameter `path_fields` does not name is not read.
    ///
    /// The rest of the input, by `spec.body`:
    ///
    /// - [`HttpBodyKind::Json`]: the body is the input object; one that does
    ///   not parse is refused with a pointer to where it went wrong. The query
    ///   string is not read. Nor is the `Content-Type` checked, unlike an RPC
    ///   body's ([`Runtime::no_input`] says why that matters): an endpoint
    ///   bound to HTTP keeps whatever contract its callers already have, so
    ///   one a cross-origin page must not reach needs its own guard, such as
    ///   a `SameSite` session cookie.
    /// - [`HttpBodyKind::Form`]: the query string, and over it a form body,
    ///   which wins per field. A body is read only when its `Content-Type`'s
    ///   media type is `application/x-www-form-urlencoded` (in any case, with
    ///   any parameters), which is how a form extractor behaves too: an
    ///   endpoint answering several methods gets a GET's empty body as often
    ///   as a POST's form.
    /// - [`HttpBodyKind::None`]: the query string only.
    pub async fn read_http_input<T: DeserializeOwned>(request: Request, spec: &HttpSpec) -> Result<T, Refusal> {
        let (mut parts, request_body) = request.into_parts();
        let params = RawPathParams::from_request_parts(&mut parts, &()).await;
        let params = match params {
            Ok(params) => params,
            Err(rejection) => return Err(Self::invalid_query(&rejection.body_text(), None).into()),
        };
        let mut path_values = Vec::new();
        for (key, raw) in params.iter().filter(|(key, _)| spec.path_fields.contains(key)) {
            let value = Self::path_value(spec.fields, key, raw)?;
            path_values.push((key.to_string(), value));
        }

        if spec.body == HttpBodyKind::Json {
            let request = Request::from_parts(parts, request_body);
            // At the envelope's status, like every other refusal here, so
            // `http_input` answers it `400`; one that could not be read keeps
            // the status of axum's rejection (`413`).
            let mut object: Map<String, Value> = Self::read_json(request, StatusCode::OK).await?;
            object.extend(path_values);
            return deserialize_value(Value::Object(object)).map_err(|parse_error| {
                parse_error
                    .map_code(|_| WireCode(R::INVALID_BODY))
                    .with_status(StatusCode::OK.as_u16())
                    .into()
            });
        }

        let mut pairs = Self::url_pairs(parts.uri.query().unwrap_or_default().as_bytes())?;
        if spec.body == HttpBodyKind::Form && declares_form(&parts.headers) {
            let request = Request::from_parts(parts, request_body);
            let read = Bytes::from_request(request, &()).await;
            let bytes = match read {
                Ok(bytes) => bytes,
                Err(rejection) => {
                    let status = rejection.into_response().status();
                    return Err(http_status(Self::invalid_query("the form body cannot be read", None), status).into());
                }
            };
            let form = Self::url_pairs(&bytes)?;
            override_pairs(&mut pairs, form);
        }
        let mut object = Self::object_from_pairs(&pairs, spec.fields)?;
        object.extend(path_values);
        Self::input_from_object(object)
    }

    /// One path parameter as the JSON value its field declares — a list
    /// field's as a list of that one value — or, for a parameter `fields` does
    /// not list, as a string.
    fn path_value(fields: &[QueryField], key: &str, raw: &str) -> Result<Value, Refusal> {
        let field = fields.iter().find(|field| field.name == key);
        let Some(field) = field else {
            return Ok(Value::String(raw.to_string()));
        };
        let value = Self::query_value(field, raw)?;
        match field.list {
            true => Ok(Value::Array(vec![value])),
            false => Ok(value),
        }
    }

    /// The session of an `@http` operation that requires one, or its refusal
    /// answered `401` unless it carries a status of its own.
    ///
    /// The handler takes the extraction's `Result` (`ctx: Result<Ctx<S::Ctx>,
    /// Refusal>`) rather than letting axum answer the refusal, so it can give
    /// the refusal a status: a caller of a route fixed elsewhere reads it. A
    /// refusal that is not about the caller — the session store unreachable —
    /// carries its own (`503`, `500`), which is kept, or such a caller reads it
    /// as a sign-out. Under `treat`'s `error-status-500` an `ApiError` refusal
    /// without a status already carries `500` and keeps it; see
    /// [`ResolveContext::from_parts`](super::ResolveContext::from_parts).
    pub fn http_session<T>(ctx: Result<Ctx<T>, Refusal>) -> Result<T, Refusal> {
        ctx.map(|Ctx(ctx)| ctx)
            .map_err(|refusal| refusal.or_status(StatusCode::UNAUTHORIZED))
    }

    /// An `@http` operation's access check, its refusal answered `403` unless
    /// it carries a status of its own.
    ///
    /// The check is the generated context trait's `require_role(..)` or
    /// `require_permission(..).await`; an RPC handler applies it with a plain
    /// `?`, since its callers read `errors[]` rather than the status line.
    /// As with [`Runtime::http_session`], a refusal already at `500` under
    /// `error-status-500` stays there.
    pub fn http_access(check: Result<(), Refusal>) -> Result<(), Refusal> {
        check.map_err(|refusal| refusal.or_status(StatusCode::FORBIDDEN))
    }
}
