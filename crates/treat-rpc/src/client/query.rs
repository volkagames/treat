//! Inputs sent in the URL — a query string, a form body or a path — for a
//! subscription, a raw request or an `@http` operation.
//!
//! The mirror of what the server decodes, driven by the same [`QueryField`]
//! table the server reads with: absent values are left out rather than sent
//! empty, only a `list` field repeats its key, and a [`QueryKind::Json`] field
//! — or any value with no query-string spelling of its own — travels as JSON
//! text.

use crate::{QueryField, QueryKind};
use serde::Serialize;
use serde_json::{Map, Value};

/// An input's fields as a query string, `?` included; `""` when nothing is
/// left to send. Refused as [`form_pairs`] refuses.
pub(crate) fn to_query(input: Map<String, Value>, fields: &[QueryField]) -> Result<String, String> {
    let pairs = form_pairs(input, fields)?;
    if pairs.is_empty() {
        return Ok(pairs);
    }
    Ok(format!("?{pairs}"))
}

/// An input's fields as `key=value` pairs joined by `&`: a query string without
/// its `?`, and the body of an `application/x-www-form-urlencoded` request.
///
/// Each field is written as its entry in `fields` says; a field the table does
/// not name — which the server will not read — as a single value that is not
/// JSON-typed.
///
/// Refuses a `null` item in a list whose kind is not [`QueryKind::Json`]: a
/// repeated key has no spelling for it, and `null` as text would reach the
/// server as the string `"null"` or not read at all.
pub(crate) fn form_pairs(input: Map<String, Value>, fields: &[QueryField]) -> Result<String, String> {
    let mut pairs: Vec<String> = Vec::new();
    for (key, value) in input {
        let field = fields.iter().find(|field| field.name == key);
        let texts = param_texts(field, value)
            .ok_or_else(|| format!("list field `{key}` has a `null` item, which no query parameter spells"))?;
        for text in texts {
            pairs.push(format!("{}={}", urlencode(&key), urlencode(&text)));
        }
    }
    Ok(pairs.join("&"))
}

/// The input's fields, for an operation that sends some of them elsewhere than
/// a JSON body. An input that is not an object has none.
pub(crate) fn input_fields<I: Serialize + ?Sized>(input: &I) -> Result<Map<String, Value>, String> {
    let value = serde_json::to_value(input).map_err(|e| e.to_string())?;
    match value {
        Value::Object(fields) => Ok(fields),
        _ => Ok(Map::new()),
    }
}

/// `path` with each `{name}` in `path_fields` replaced by that field of
/// `input`, which is removed from it: a path parameter travels in the path
/// only. The value is spelled as a query parameter of its kind would be, which
/// is how the server reads a path parameter back.
///
/// A `list` field's segment is its one item, spelled as that item's kind: the
/// server reads a list path parameter as a list of the one value the segment
/// carries. An array of any other length has no segment that says it.
///
/// Refuses a field that is absent or `null`, and a value that is empty, `.` or
/// `..`: URL parsing removes a `.` or `..` segment, escaped or not, so the
/// request would reach another route, and an empty one reaches none.
pub(crate) fn fill_path(
    path: &str,
    path_fields: &[&str],
    input: &mut Map<String, Value>,
    fields: &[QueryField],
) -> Result<String, String> {
    let mut url_path = path.to_string();
    for name in path_fields {
        let field = fields.iter().find(|field| field.name == *name);
        let list = field.is_some_and(|field| field.list);
        let value = input.remove(*name);
        let value = match value {
            Some(Value::Array(items)) if list => only_item(name, items)?,
            Some(value) => value,
            None => Value::Null,
        };
        if value.is_null() {
            return Err(format!("path parameter `{name}` is missing"));
        }
        let text = param_text(field.map(|field| field.kind), value);
        if matches!(text.as_str(), "" | "." | "..") {
            return Err(format!(
                "path parameter `{name}` cannot be `{text}`: no URL carries it as a segment"
            ));
        }
        let placeholder = ["{", name, "}"].concat();
        url_path = url_path.replace(&placeholder, &path_segment(&text));
    }
    Ok(url_path)
}

/// The one item of a `list` path parameter's array.
fn only_item(name: &str, items: Vec<Value>) -> Result<Value, String> {
    match <[Value; 1]>::try_from(items) {
        Ok([item]) => Ok(item),
        Err(items) => {
            Err(format!(
                "list path parameter `{name}` holds {} items: a segment carries exactly one",
                items.len()
            ))
        }
    }
}

/// The texts one field's value travels as: none for `null`, one per item for a
/// `list` field's array, else one.
///
/// `None` for a list holding `null` where its kind is not
/// [`QueryKind::Json`]: only JSON text spells `null`, so such an item has no
/// text the server would read back as what was sent.
fn param_texts(field: Option<&QueryField>, value: Value) -> Option<Vec<String>> {
    let kind = field.map(|field| field.kind);
    let list = field.is_some_and(|field| field.list);
    match value {
        Value::Null => Some(Vec::new()),
        Value::Array(items) if list => {
            items
                .into_iter()
                .map(|item| {
                    match (kind, item) {
                        (Some(QueryKind::Json), item) => Some(param_text(kind, item)),
                        (_, Value::Null) => None,
                        (_, item) => Some(param_text(kind, item)),
                    }
                })
                .collect()
        }
        value => Some(vec![param_text(kind, value)]),
    }
}

/// One parameter's text. A [`QueryKind::Json`] value is always JSON text — a
/// string too, quoted, since the server parses the parameter as JSON. Any
/// other string travels as itself, which is what the server reads it back as,
/// and anything else as its JSON text: a number or a boolean is its own
/// spelling, and an object or an array has no other.
fn param_text(kind: Option<QueryKind>, value: Value) -> String {
    match (kind, value) {
        (Some(QueryKind::Json), value) => value.to_string(),
        (_, Value::String(text)) => text,
        (_, value) => value.to_string(),
    }
}

/// Percent-encode everything a query parameter may not carry literally, a
/// space as `+`.
pub(crate) fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(byte)),
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Percent-encode a path segment: as [`urlencode`], but a space is `%20`,
/// since `+` in a path is a literal plus.
pub(crate) fn path_segment(value: &str) -> String {
    // A `+` in `value` was encoded as `%2B`, so every `+` left is a space.
    urlencode(value).replace('+', "%20")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const fn field(name: &'static str, kind: QueryKind, list: bool) -> QueryField {
        QueryField { name, kind, list }
    }

    static FIELDS: &[QueryField] = &[
        field("q", QueryKind::Str, false),
        field("n", QueryKind::Int, false),
        field("flag", QueryKind::Bool, false),
        field("tags", QueryKind::Str, true),
        field("j", QueryKind::Json, false),
        field("js", QueryKind::Json, true),
        field("pair", QueryKind::Str, false),
    ];

    fn query(input: Value) -> String {
        to_query(input_fields(&input).unwrap_or_default(), FIELDS).unwrap_or_else(|e| e)
    }

    /// The pairs of a query, sorted: field order is serde_json's map order,
    /// which a `preserve_order` feature elsewhere in the build changes.
    fn sorted(query: &str) -> Vec<String> {
        let mut pairs: Vec<String> = query
            .strip_prefix('?')
            .unwrap_or_default()
            .split('&')
            .map(str::to_string)
            .collect();
        pairs.sort_unstable();
        pairs
    }

    #[test]
    fn urlencode_keeps_unreserved_only() {
        assert_eq!(urlencode("AZaz09-_.~"), "AZaz09-_.~");
        assert_eq!(urlencode("a b"), "a+b");
        assert_eq!(urlencode("a+b&c=d/e?f#g%h"), "a%2Bb%26c%3Dd%2Fe%3Ff%23g%25h");
        assert_eq!(urlencode("{x}"), "%7Bx%7D");
        assert_eq!(urlencode(""), "");
    }

    #[test]
    fn urlencode_writes_non_ascii_as_its_utf8_bytes() {
        assert_eq!(urlencode("é"), "%C3%A9");
        assert_eq!(urlencode("я"), "%D1%8F");
        assert_eq!(urlencode("😀"), "%F0%9F%98%80");
    }

    #[test]
    fn path_segment_spells_a_space_as_percent_20() {
        assert_eq!(path_segment("a b"), "a%20b");
        assert_eq!(path_segment("a+b"), "a%2Bb");
        assert_eq!(path_segment("a/b"), "a%2Fb");
    }

    #[test]
    fn to_query_leaves_out_null_and_repeats_a_list_only() {
        let input = json!({"q": null, "tags": ["x", "y z"], "n": 3, "flag": true});
        let query = query(input);
        assert_eq!(sorted(&query), ["flag=true", "n=3", "tags=x", "tags=y+z"], "{query}");
        assert!(query.contains("tags=x&tags=y+z"), "a list keeps its order: {query}");
    }

    #[test]
    fn a_non_list_array_is_one_json_parameter() {
        assert_eq!(query(json!({"pair": [1, 2]})), "?pair=%5B1%2C2%5D");
        // A field the table does not name is not a list either.
        assert_eq!(query(json!({"other": ["a"]})), "?other=%5B%22a%22%5D");
    }

    #[test]
    fn a_json_field_is_always_json_text() {
        assert_eq!(query(json!({"j": "abc"})), "?j=%22abc%22", "a string is quoted");
        assert_eq!(query(json!({"j": 5})), "?j=5");
        assert_eq!(query(json!({"j": [1, 2]})), "?j=%5B1%2C2%5D", "an array is one value");
        assert_eq!(query(json!({"j": {"k": 1}})), "?j=%7B%22k%22%3A1%7D");
        assert_eq!(
            query(json!({"js": ["a", {"k": 1}]})),
            "?js=%22a%22&js=%7B%22k%22%3A1%7D",
            "a JSON list repeats its key, one JSON text per item"
        );
    }

    #[test]
    fn a_string_field_is_its_text() {
        assert_eq!(query(json!({"q": "hello"})), "?q=hello");
        assert_eq!(
            query(json!({"q": {"k": 1}})),
            "?q=%7B%22k%22%3A1%7D",
            "no other spelling"
        );
    }

    #[test]
    fn to_query_of_nothing_is_empty() {
        assert_eq!(query(json!({})), "");
        assert_eq!(query(json!({"q": null, "tags": []})), "");
        assert_eq!(query(json!(null)), "");
        assert_eq!(query(json!(5)), "");
        let none = input_fields(&crate::client::NoInput).unwrap_or_default();
        assert_eq!(to_query(none, &[]).as_deref(), Ok(""));
    }

    #[test]
    fn form_pairs_is_the_query_without_its_mark() {
        let fields = |value| input_fields(&value).unwrap_or_default();
        assert_eq!(form_pairs(fields(json!({"q": "1 2"})), FIELDS).as_deref(), Ok("q=1+2"));
        assert_eq!(
            form_pairs(fields(json!({"j": "x"})), FIELDS).as_deref(),
            Ok("j=%22x%22")
        );
        assert_eq!(form_pairs(fields(json!({})), FIELDS).as_deref(), Ok(""));
    }

    #[test]
    fn a_null_item_is_refused_in_a_list_only_json_spells() {
        let fields = |value| input_fields(&value).unwrap_or_default();
        let refused = to_query(fields(json!({"tags": ["x", null]})), FIELDS);
        assert_eq!(
            refused,
            Err("list field `tags` has a `null` item, which no query parameter spells".to_string())
        );
        assert!(form_pairs(fields(json!({"tags": [null]})), FIELDS).is_err());
        // A JSON list spells it, and a non-list array is one JSON text.
        assert_eq!(query(json!({"js": [null, 1]})), "?js=null&js=1");
        assert_eq!(query(json!({"pair": [null]})), "?pair=%5Bnull%5D");
    }

    #[test]
    fn fill_path_moves_fields_into_the_path() {
        let mut input = input_fields(&json!({"q": "a b/c", "n": 7, "rest": "x"})).unwrap_or_default();
        let path = fill_path("/api/{q}/items/{n}", &["q", "n"], &mut input, FIELDS);
        assert_eq!(path.as_deref(), Ok("/api/a%20b%2Fc/items/7"));
        assert_eq!(Value::Object(input), json!({"rest": "x"}));
    }

    #[test]
    fn fill_path_spells_a_json_field_as_json() {
        let mut input = input_fields(&json!({"j": "x"})).unwrap_or_default();
        let path = fill_path("/x/{j}", &["j"], &mut input, FIELDS);
        assert_eq!(path.as_deref(), Ok("/x/%22x%22"));
    }

    #[test]
    fn fill_path_refuses_what_no_segment_carries() {
        for value in [json!(""), json!("."), json!("..")] {
            let mut input = input_fields(&json!({ "id": value })).unwrap_or_default();
            let path = fill_path("/x/{id}", &["id"], &mut input, &[]);
            assert!(path.is_err(), "{value}: {path:?}");
        }
        // Escaped, they are still segments a URL parser removes; `...` is not.
        let mut input = input_fields(&json!({"id": "..."})).unwrap_or_default();
        assert_eq!(fill_path("/x/{id}", &["id"], &mut input, &[]).as_deref(), Ok("/x/..."));
    }

    #[test]
    fn fill_path_refuses_a_missing_parameter() {
        for value in [json!({}), json!({"id": null})] {
            let mut input = input_fields(&value).unwrap_or_default();
            let path = fill_path("/x/{id}", &["id"], &mut input, &[]);
            assert_eq!(path, Err("path parameter `id` is missing".to_string()));
        }
    }

    #[test]
    fn fill_path_spells_a_list_field_as_its_one_item() {
        let fields = [field("tags", QueryKind::Str, true), field("ns", QueryKind::Int, true)];
        let mut input = input_fields(&json!({"tags": ["a b"], "ns": [7]})).unwrap_or_default();
        let path = fill_path("/x/{tags}/{ns}", &["tags", "ns"], &mut input, &fields);
        assert_eq!(path.as_deref(), Ok("/x/a%20b/7"));

        let mut input = input_fields(&json!({"js": [{"k": 1}]})).unwrap_or_default();
        let path = fill_path("/x/{js}", &["js"], &mut input, FIELDS);
        assert_eq!(path.as_deref(), Ok("/x/%7B%22k%22%3A1%7D"));
    }

    #[test]
    fn fill_path_refuses_a_list_field_of_other_than_one_item() {
        for tags in [json!([]), json!(["a", "b"])] {
            let mut input = input_fields(&json!({ "tags": tags })).unwrap_or_default();
            let path = fill_path("/x/{tags}", &["tags"], &mut input, FIELDS);
            assert!(path.is_err(), "{tags}: {path:?}");
        }
        let mut input = input_fields(&json!({"tags": [null]})).unwrap_or_default();
        let path = fill_path("/x/{tags}", &["tags"], &mut input, FIELDS);
        assert_eq!(path, Err("path parameter `tags` is missing".to_string()));
    }

    #[test]
    fn input_fields_of_a_non_object_is_empty() {
        assert_eq!(input_fields(&json!([1])), Ok(Map::new()));
        assert_eq!(input_fields(&crate::client::NoInput), Ok(Map::new()));
    }
}
