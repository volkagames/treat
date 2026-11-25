//! `@http` operations: a route fixed outside the schema.

use super::call::{json_body, receive, send};
use super::query::{fill_path, form_pairs, input_fields, to_query};
use super::{ClientError, ClientResult, RpcClient};
use crate::{HttpBodyKind, HttpSpec};
use serde::Serialize;
use serde::de::DeserializeOwned;

impl RpcClient {
    /// Call an `@http` operation as its [`HttpSpec`] describes it — the same
    /// `static` its server reads the request with.
    ///
    /// The request goes to `spec.path` under [`HttpSpec::request_method`]: the
    /// method the route is mounted under, or for `ANY` `GET` without a body and
    /// `POST` with one. Each `{name}` segment `spec.path_fields` lists is
    /// filled from that input field, which then travels in the path only; the
    /// remaining fields travel as `spec.body` says. `spec.fields` says how each
    /// field is spelled in the path, a query string or a form body: a
    /// JSON-typed field as JSON text, and only a list by repeating its key.
    ///
    /// Under a method that carries no body (`GET`, `DELETE`) a
    /// [`HttpBodyKind::Form`] input goes in the query string instead: the
    /// server reads a form operation's query string under the same table, so
    /// it reads the same input, and the request keeps to what HTTP defines
    /// for those methods. A [`HttpBodyKind::Json`] input is still the JSON
    /// body there: the server reads it, and its query-string fallback for a
    /// body-less call reads only the fields the table names, so moving the
    /// input to the URL could drop some.
    ///
    /// A success is read as the envelope or, with `spec.envelope` false, as
    /// the value itself. A failure is the envelope either way, under the status
    /// the failure carries — `errors[]` decides, so a status the service chose
    /// does not hide the codes it sent.
    ///
    /// An input that does not serialize, a path parameter that is missing or
    /// would not stay one segment of the URL (empty, `.`, `..`), or a `null`
    /// item in a list that is not JSON-typed is [`ClientError::Encode`], and
    /// nothing is sent.
    pub async fn call_http<I, O, C>(&self, spec: &HttpSpec, input: &I) -> ClientResult<O, C>
    where
        I: Serialize + ?Sized,
        O: DeserializeOwned,
    {
        let mut input = input_fields(input).map_err(ClientError::Encode)?;
        let url_path = fill_path(spec.path, spec.path_fields, &mut input, spec.fields).map_err(ClientError::Encode)?;
        let url = self.url(&url_path);
        // Parsed rather than matched: every `HttpMethod` spelling is a valid
        // method token, so this does not fail, and `request_method` never
        // answers `ANY` for it to be sent.
        let method = reqwest::Method::from_bytes(spec.request_method().as_str().as_bytes())
            .map_err(|e| ClientError::Encode(e.to_string()))?;

        // `GET` and `DELETE`: the methods under which the server reads a JSON
        // operation called without a body from its query string.
        let bodiless = HttpBodyKind::default_for(spec.request_method()) == HttpBodyKind::None;
        let request = match spec.body {
            HttpBodyKind::None => self.query_request(method, &url, input, spec)?,
            HttpBodyKind::Form if bodiless => self.query_request(method, &url, input, spec)?,
            HttpBodyKind::Json => json_body(self.http.request(method, url), &input)?,
            HttpBodyKind::Form => {
                let form = form_pairs(input, spec.fields).map_err(ClientError::Encode)?;
                self.http
                    .request(method, url)
                    .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(form)
            }
        };
        let response = send(request).await?;
        receive(response, spec.envelope).await
    }

    /// A request to `url` carrying `input` in its query string, spelled as
    /// `spec.fields` says.
    fn query_request<C>(
        &self,
        method: reqwest::Method,
        url: &str,
        input: serde_json::Map<String, serde_json::Value>,
        spec: &HttpSpec,
    ) -> Result<reqwest::RequestBuilder, ClientError<C>> {
        let query = to_query(input, spec.fields).map_err(ClientError::Encode)?;
        Ok(self.http.request(method, format!("{url}{query}")))
    }
}
