//! How an HTTP-bound operation sits on the wire: its method, its path, where
//! its input travels and whether its success is the envelope.
//!
//! One description for both ends of the wire, so a server and a client
//! generated from the same schema agree on the encoding by construction: a
//! generator emits one `static` [`HttpSpec`] per operation and hands the same
//! one to the server's reader and the client's caller.

use super::QueryField;

/// Where an HTTP-bound operation carries the input fields its path does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpBodyKind {
    /// No body: the fields travel in the query string.
    None,
    /// The input object as a JSON body.
    Json,
    /// An `application/x-www-form-urlencoded` body. A server reads the query
    /// string as well and lets the body win: a field present in both takes the
    /// body's value, so a client is free to send every field in the body.
    Form,
}

impl HttpBodyKind {
    /// The body kind an `@http` directive without a `body:` argument has:
    /// [`HttpBodyKind::Json`] under a method that conventionally carries a
    /// body (`POST`, `PUT`, `PATCH`), [`HttpBodyKind::None`] under the rest
    /// (`GET`, `DELETE`, `ANY`).
    ///
    /// ```
    /// use treat_core::rpc::{HttpBodyKind, HttpMethod};
    ///
    /// const PATCHED: HttpBodyKind = HttpBodyKind::default_for(HttpMethod::Patch);
    /// assert_eq!(PATCHED, HttpBodyKind::Json);
    /// assert_eq!(HttpBodyKind::default_for(HttpMethod::Any), HttpBodyKind::None);
    /// ```
    pub const fn default_for(method: HttpMethod) -> Self {
        match method {
            HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch => Self::Json,
            HttpMethod::Get | HttpMethod::Delete | HttpMethod::Any => Self::None,
        }
    }
}

/// The method an HTTP-bound operation is mounted under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
    /// `PUT`.
    Put,
    /// `PATCH`.
    Patch,
    /// `DELETE`.
    Delete,
    /// Every method, as an existing endpoint that never checked its method
    /// does. The input is read the same way whatever the method: a request
    /// without a body simply has none to read.
    Any,
}

impl HttpMethod {
    /// The method as HTTP spells it, upper-case; `ANY` for [`HttpMethod::Any`],
    /// which is [`route::ANY`](super::route::ANY), the method of an operation
    /// table's row that answers every method.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Any => super::route::ANY,
        }
    }
}

/// Everything about an HTTP-bound operation that both ends of the wire read:
/// the route, the field table, where the input travels and how a success is
/// written.
///
/// Plain data with public fields, so it is a `static` built in a constant
/// expression, one per operation, which the server's `Runtime::http_input` and
/// `Runtime::respond_http` and the client's `RpcClient::call_http` all take.
/// The `const` constructor and builders spell out only what differs from the
/// directive's defaults:
///
/// ```
/// use treat_core::rpc::{HttpBodyKind, HttpMethod, HttpSpec, QueryField};
///
/// static USER: HttpSpec = HttpSpec::get("/api/user/{user_id}")
///     .fields(&[QueryField::str("user_id")])
///     .path_fields(&["user_id"])
///     .bare();
/// assert_eq!(USER.request_method(), HttpMethod::Get);
/// assert_eq!((USER.body, USER.envelope), (HttpBodyKind::None, false));
/// ```
///
/// The struct literal is the same value, every field written out:
///
/// ```
/// use treat_core::rpc::{HttpBodyKind, HttpMethod, HttpSpec, QueryField, QueryKind};
///
/// static USER: HttpSpec = HttpSpec {
///     method: HttpMethod::Get,
///     path: "/api/user/{user_id}",
///     fields: &[QueryField { name: "user_id", kind: QueryKind::Str, list: false }],
///     path_fields: &["user_id"],
///     body: HttpBodyKind::None,
///     envelope: false,
/// };
/// const BUILT: HttpSpec = HttpSpec::get("/api/user/{user_id}")
///     .fields(&[QueryField::str("user_id")])
///     .path_fields(&["user_id"])
///     .bare();
/// assert_eq!(USER, BUILT);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HttpSpec {
    /// The method the route is mounted under.
    pub method: HttpMethod,
    /// The route as the router mounts it, `{name}` segments and all.
    pub path: &'static str,
    /// How each input field is spelled in a query string, a form body or a
    /// path segment. A field the table does not name is not read from a query
    /// string or a form; a path parameter it does not name is read as a string.
    pub fields: &'static [QueryField],
    /// The `{name}` segments of `path` that are input fields. They travel in
    /// the path only, and win over a field of the same name anywhere else: the
    /// path is what the route matched.
    pub path_fields: &'static [&'static str],
    /// Where the fields the path does not carry travel.
    pub body: HttpBodyKind,
    /// Whether a success is the `{"data": ...}` envelope (`true`) or the value
    /// itself. A failure is the envelope either way.
    pub envelope: bool,
}

impl HttpSpec {
    /// An operation mounted under `method` at `path`, with the defaults of
    /// `@http`: no field table and no path fields, the body kind `method`
    /// implies ([`HttpBodyKind::default_for`]) and the envelope on.
    ///
    /// The builders below change one field each, so a generated `static`
    /// states only what its directive does.
    ///
    /// ```
    /// use treat_core::rpc::{HttpBodyKind, HttpMethod, HttpSpec};
    ///
    /// static CREATE: HttpSpec = HttpSpec::new(HttpMethod::Post, "/api/items");
    /// assert_eq!(CREATE.body, HttpBodyKind::Json);
    /// assert!(CREATE.envelope);
    /// assert!(CREATE.fields.is_empty() && CREATE.path_fields.is_empty());
    /// ```
    pub const fn new(method: HttpMethod, path: &'static str) -> Self {
        Self {
            method,
            path,
            fields: &[],
            path_fields: &[],
            body: HttpBodyKind::default_for(method),
            envelope: true,
        }
    }

    /// [`HttpSpec::new`] under `GET`: the input in the query string.
    ///
    /// ```
    /// use treat_core::rpc::{HttpBodyKind, HttpMethod, HttpSpec};
    ///
    /// static LIST: HttpSpec = HttpSpec::get("/api/items");
    /// assert_eq!((LIST.method, LIST.body), (HttpMethod::Get, HttpBodyKind::None));
    /// ```
    pub const fn get(path: &'static str) -> Self {
        Self::new(HttpMethod::Get, path)
    }

    /// [`HttpSpec::new`] under `POST`: the input as a JSON body.
    ///
    /// ```
    /// use treat_core::rpc::{HttpBodyKind, HttpMethod, HttpSpec};
    ///
    /// static CREATE: HttpSpec = HttpSpec::post("/api/items");
    /// assert_eq!((CREATE.method, CREATE.body), (HttpMethod::Post, HttpBodyKind::Json));
    /// ```
    pub const fn post(path: &'static str) -> Self {
        Self::new(HttpMethod::Post, path)
    }

    /// [`HttpSpec::new`] under `PUT`: the input as a JSON body.
    ///
    /// ```
    /// use treat_core::rpc::{HttpBodyKind, HttpMethod, HttpSpec};
    ///
    /// static REPLACE: HttpSpec = HttpSpec::put("/api/items/{id}");
    /// assert_eq!((REPLACE.method, REPLACE.body), (HttpMethod::Put, HttpBodyKind::Json));
    /// ```
    pub const fn put(path: &'static str) -> Self {
        Self::new(HttpMethod::Put, path)
    }

    /// [`HttpSpec::new`] under `PATCH`: the input as a JSON body.
    ///
    /// ```
    /// use treat_core::rpc::{HttpBodyKind, HttpMethod, HttpSpec};
    ///
    /// static UPDATE: HttpSpec = HttpSpec::patch("/api/items/{id}");
    /// assert_eq!((UPDATE.method, UPDATE.body), (HttpMethod::Patch, HttpBodyKind::Json));
    /// ```
    pub const fn patch(path: &'static str) -> Self {
        Self::new(HttpMethod::Patch, path)
    }

    /// [`HttpSpec::new`] under `DELETE`: the input in the query string.
    ///
    /// ```
    /// use treat_core::rpc::{HttpBodyKind, HttpMethod, HttpSpec};
    ///
    /// static REMOVE: HttpSpec = HttpSpec::delete("/api/items/{id}");
    /// assert_eq!((REMOVE.method, REMOVE.body), (HttpMethod::Delete, HttpBodyKind::None));
    /// ```
    pub const fn delete(path: &'static str) -> Self {
        Self::new(HttpMethod::Delete, path)
    }

    /// [`HttpSpec::new`] under every method ([`HttpMethod::Any`]): the input
    /// in the query string.
    ///
    /// ```
    /// use treat_core::rpc::{HttpBodyKind, HttpMethod, HttpSpec};
    ///
    /// static INFO: HttpSpec = HttpSpec::any("/api/info").body(HttpBodyKind::Form);
    /// assert_eq!((INFO.method, INFO.body), (HttpMethod::Any, HttpBodyKind::Form));
    /// ```
    pub const fn any(path: &'static str) -> Self {
        Self::new(HttpMethod::Any, path)
    }

    /// The same spec with `fields` as its field table.
    ///
    /// ```
    /// use treat_core::rpc::{HttpSpec, QueryField};
    ///
    /// static FIELDS: &[QueryField] = &[QueryField::int("limit"), QueryField::str("tag").list()];
    /// static FIND: HttpSpec = HttpSpec::get("/api/users").fields(FIELDS);
    /// assert_eq!(FIND.fields.len(), 2);
    /// ```
    #[must_use]
    pub const fn fields(self, fields: &'static [QueryField]) -> Self {
        Self { fields, ..self }
    }

    /// The same spec with `path_fields` as the `{name}` segments of its path
    /// that are input fields.
    ///
    /// ```
    /// use treat_core::rpc::HttpSpec;
    ///
    /// static USER: HttpSpec = HttpSpec::get("/api/users/{id}").path_fields(&["id"]);
    /// assert_eq!(USER.path_fields, &["id"]);
    /// ```
    #[must_use]
    pub const fn path_fields(self, path_fields: &'static [&'static str]) -> Self {
        Self { path_fields, ..self }
    }

    /// The same spec with the input's other fields travelling as `body` says,
    /// whatever its method implies.
    ///
    /// ```
    /// use treat_core::rpc::{HttpBodyKind, HttpSpec};
    ///
    /// static LOGIN: HttpSpec = HttpSpec::post("/login").body(HttpBodyKind::Form);
    /// assert_eq!(LOGIN.body, HttpBodyKind::Form);
    /// ```
    #[must_use]
    pub const fn body(self, body: HttpBodyKind) -> Self {
        Self { body, ..self }
    }

    /// The same spec answering a success with the value itself rather than
    /// the `{"data": ...}` envelope (`envelope: false`). A failure is the
    /// envelope either way.
    ///
    /// ```
    /// use treat_core::rpc::HttpSpec;
    ///
    /// static USER: HttpSpec = HttpSpec::get("/api/user").bare();
    /// assert!(!USER.envelope);
    /// ```
    #[must_use]
    pub const fn bare(self) -> Self {
        Self {
            envelope: false,
            ..self
        }
    }

    /// The method a client sends: the one the route is mounted under, or for
    /// [`HttpMethod::Any`] `GET` when the input travels without a body and
    /// `POST` when it travels in one. Never [`HttpMethod::Any`].
    ///
    /// An `ANY` route reads the same input whatever the method, so the choice
    /// is the client's; `GET` is what a body-less request is, and a body needs
    /// a method that conventionally carries one.
    pub const fn request_method(&self) -> HttpMethod {
        match (self.method, self.body) {
            (HttpMethod::Any, HttpBodyKind::None) => HttpMethod::Get,
            (HttpMethod::Any, HttpBodyKind::Json | HttpBodyKind::Form) => HttpMethod::Post,
            (method, _) => method,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn spec(method: HttpMethod, body: HttpBodyKind) -> HttpSpec {
        HttpSpec {
            method,
            path: "/x",
            fields: &[],
            path_fields: &[],
            body,
            envelope: true,
        }
    }

    #[test]
    fn any_is_sent_as_get_without_a_body_and_post_with_one() {
        assert_eq!(
            spec(HttpMethod::Any, HttpBodyKind::None).request_method(),
            HttpMethod::Get
        );
        assert_eq!(
            spec(HttpMethod::Any, HttpBodyKind::Form).request_method(),
            HttpMethod::Post
        );
        assert_eq!(
            spec(HttpMethod::Any, HttpBodyKind::Json).request_method(),
            HttpMethod::Post
        );
    }

    #[test]
    fn new_takes_the_directive_s_defaults() {
        let cases = [
            (HttpMethod::Get, HttpSpec::get("/x"), HttpBodyKind::None),
            (HttpMethod::Post, HttpSpec::post("/x"), HttpBodyKind::Json),
            (HttpMethod::Put, HttpSpec::put("/x"), HttpBodyKind::Json),
            (HttpMethod::Patch, HttpSpec::patch("/x"), HttpBodyKind::Json),
            (HttpMethod::Delete, HttpSpec::delete("/x"), HttpBodyKind::None),
            (HttpMethod::Any, HttpSpec::any("/x"), HttpBodyKind::None),
        ];
        for (method, shorthand, body) in cases {
            assert_eq!(HttpSpec::new(method, "/x"), spec(method, body), "{method:?}");
            assert_eq!(shorthand, spec(method, body), "{method:?}");
        }
    }

    #[test]
    fn each_builder_changes_its_own_field() {
        static FIELDS: &[QueryField] = &[QueryField::int("id"), QueryField::str("tag").list()];
        let built = HttpSpec::get("/x")
            .fields(FIELDS)
            .path_fields(&["id"])
            .body(HttpBodyKind::Form)
            .bare();
        let literal = HttpSpec {
            method: HttpMethod::Get,
            path: "/x",
            fields: FIELDS,
            path_fields: &["id"],
            body: HttpBodyKind::Form,
            envelope: false,
        };
        assert_eq!(built, literal);
    }

    #[test]
    fn a_bound_method_is_sent_as_itself() {
        for method in [
            HttpMethod::Get,
            HttpMethod::Post,
            HttpMethod::Put,
            HttpMethod::Patch,
            HttpMethod::Delete,
        ] {
            for body in [HttpBodyKind::None, HttpBodyKind::Json, HttpBodyKind::Form] {
                assert_eq!(spec(method, body).request_method(), method);
            }
        }
    }

    #[test]
    fn methods_are_spelled_as_http_and_the_route_table_spells_them() {
        assert_eq!(HttpMethod::Get.as_str(), "GET");
        assert_eq!(HttpMethod::Delete.as_str(), "DELETE");
        assert_eq!(HttpMethod::Any.as_str(), crate::rpc::route::ANY);
    }
}
