//! How an input field travels as text: in a query string, a form body or a
//! path segment.
//!
//! One table for both ends of the wire. A server rebuilds an input from
//! `key=value` text with it, and a client writes the text with the same one, so
//! a field the schema types as JSON is JSON text in both directions and a list
//! repeats its key in both — the two sides cannot disagree about a spelling the
//! bytes alone do not settle.

/// How one query-string parameter is decoded before serde sees it, and how a
/// client writes it.
///
/// A query string carries only text, so a value's JSON type comes from the
/// schema rather than from the bytes: `column=2` is the number `2` for an
/// `Int!` field and the string `"2"` for a `String!` one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryKind {
    /// The text itself, as a JSON string.
    Str,
    /// A JSON integer (`i64`).
    Int,
    /// A JSON number; a value with no JSON spelling (`NaN`, `inf`) is refused.
    Float,
    /// `true` or `false`, exactly.
    Bool,
    /// Anything with no query-string spelling of its own — an object, or the
    /// unconstrained `JSON` scalar — sent as JSON text in the parameter.
    ///
    /// Always JSON text, a string included: the string `abc` travels as
    /// `"abc"`, quotes and all, since the server parses the parameter as JSON
    /// and `abc` alone is not.
    Json,
}

impl QueryKind {
    /// How a refusal names a value of this kind: `` `x` is not a valid integer ``.
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Str => "string",
            Self::Int => "integer",
            Self::Float => "number",
            Self::Bool => "boolean",
            Self::Json => "JSON value",
        }
    }
}

/// One input field as the query string carries it.
///
/// Plain data with public fields, so a generated table of them is a `static`
/// that the server and the client of an operation share. The `const`
/// constructors keep such a table to one line per field:
///
/// ```
/// use treat_core::rpc::QueryField;
///
/// static FIND_FIELDS: &[QueryField] = &[
///     QueryField::int("limit"),
///     QueryField::str("tags").list(),
///     QueryField::json("filter"),
/// ];
/// assert!(FIND_FIELDS[1].list);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QueryField {
    /// The field's name in the input object, which is also the parameter's.
    pub name: &'static str,
    /// The JSON type the parameter's text is converted to.
    pub kind: QueryKind,
    /// A list repeats its key, which is what `URLSearchParams` writes and what
    /// reading with `getAll` expects. Only a list does: a value of a field that
    /// is not one travels under one key, as JSON text if it is an array.
    ///
    /// An empty list repeats its key no times, so it travels as nothing at
    /// all: a client sends none of it, and a server leaves the field out of
    /// the input it rebuilds, as it does any absent parameter. The input's
    /// list field needs `#[serde(default)]` to read back empty rather than be
    /// refused as a missing field (an `Option` list reads back as `None`).
    pub list: bool,
}

impl QueryField {
    /// A field of the given kind that is not a list.
    ///
    /// The general form of the per-kind constructors below, for a caller that
    /// holds the kind as a value; [`QueryField::list`] makes it a list.
    ///
    /// ```
    /// use treat_core::rpc::{QueryField, QueryKind};
    ///
    /// static LIMIT: QueryField = QueryField::new("limit", QueryKind::Int);
    /// assert_eq!(LIMIT, QueryField::int("limit"));
    /// ```
    pub const fn new(name: &'static str, kind: QueryKind) -> Self {
        Self {
            name,
            kind,
            list: false,
        }
    }

    /// A [`QueryKind::Str`] field: the parameter's text is the value.
    ///
    /// ```
    /// use treat_core::rpc::{QueryField, QueryKind};
    ///
    /// static EMAIL: QueryField = QueryField::str("email");
    /// assert_eq!((EMAIL.name, EMAIL.kind, EMAIL.list), ("email", QueryKind::Str, false));
    /// ```
    pub const fn str(name: &'static str) -> Self {
        Self::new(name, QueryKind::Str)
    }

    /// A [`QueryKind::Int`] field: the parameter's text is a JSON integer.
    ///
    /// ```
    /// use treat_core::rpc::{QueryField, QueryKind};
    ///
    /// static LIMIT: QueryField = QueryField::int("limit");
    /// assert_eq!(LIMIT.kind, QueryKind::Int);
    /// ```
    pub const fn int(name: &'static str) -> Self {
        Self::new(name, QueryKind::Int)
    }

    /// A [`QueryKind::Float`] field: the parameter's text is a JSON number.
    ///
    /// ```
    /// use treat_core::rpc::{QueryField, QueryKind};
    ///
    /// static RATIO: QueryField = QueryField::float("ratio");
    /// assert_eq!(RATIO.kind, QueryKind::Float);
    /// ```
    pub const fn float(name: &'static str) -> Self {
        Self::new(name, QueryKind::Float)
    }

    /// A [`QueryKind::Bool`] field: the parameter's text is `true` or `false`.
    ///
    /// ```
    /// use treat_core::rpc::{QueryField, QueryKind};
    ///
    /// static VERBOSE: QueryField = QueryField::bool("verbose");
    /// assert_eq!(VERBOSE.kind, QueryKind::Bool);
    /// ```
    pub const fn bool(name: &'static str) -> Self {
        Self::new(name, QueryKind::Bool)
    }

    /// A [`QueryKind::Json`] field: the parameter is JSON text, a string
    /// included.
    ///
    /// ```
    /// use treat_core::rpc::{QueryField, QueryKind};
    ///
    /// static FILTER: QueryField = QueryField::json("filter");
    /// assert_eq!(FILTER.kind, QueryKind::Json);
    /// ```
    pub const fn json(name: &'static str) -> Self {
        Self::new(name, QueryKind::Json)
    }

    /// The same field as a list, which repeats its key: `tags=a&tags=b`.
    ///
    /// ```
    /// use treat_core::rpc::QueryField;
    ///
    /// static TAGS: QueryField = QueryField::str("tags").list();
    /// assert!(TAGS.list);
    /// ```
    #[must_use]
    pub const fn list(self) -> Self {
        Self { list: true, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_constructor_is_its_struct_literal() {
        let cases = [
            (QueryField::str("f"), QueryKind::Str),
            (QueryField::int("f"), QueryKind::Int),
            (QueryField::float("f"), QueryKind::Float),
            (QueryField::bool("f"), QueryKind::Bool),
            (QueryField::json("f"), QueryKind::Json),
        ];
        for (field, kind) in cases {
            let literal = QueryField {
                name: "f",
                kind,
                list: false,
            };
            assert_eq!(field, literal);
            assert_eq!(QueryField::new("f", kind), literal);
            assert_eq!(field.list(), QueryField { list: true, ..literal });
        }
    }
}
