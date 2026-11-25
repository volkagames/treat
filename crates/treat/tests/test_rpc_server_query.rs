//! Inputs read from the URL: `url_text`, `url_pairs`, `input_from_pairs`,
//! `query_value`, `override_pairs`, and `query_input`'s validation.
#![cfg(all(feature = "rpc-server", feature = "derive"))]

mod rpc_server;

use rpc_server::{FIND_FIELDS, INVALID, Rt, code, read};
use serde::Deserialize;
use treat::rpc::server::{QueryField, QueryKind, override_pairs};
use validator::Validate;

fn text(encoded: &[u8]) -> Option<String> {
    Rt::url_text(encoded).ok()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn url_text_decodes_escapes_in_either_case() {
    assert_eq!(text(b"%41%4a%4A%2b"), Some("AJJ+".to_string()));
    assert_eq!(text(b"%C3%A9%c3%a9"), Some("éé".to_string()));
    assert_eq!(text(b"a+b%20c"), Some("a b c".to_string()), "`+` is a space");
    assert_eq!(text(b""), Some(String::new()));
}

#[test]
fn url_text_keeps_a_percent_without_two_hex_digits() {
    assert_eq!(text(b"%"), Some("%".to_string()));
    assert_eq!(text(b"100%"), Some("100%".to_string()));
    assert_eq!(text(b"%4"), Some("%4".to_string()));
    assert_eq!(text(b"%G1"), Some("%G1".to_string()));
    assert_eq!(text(b"%zz%41"), Some("%zzA".to_string()));
    assert_eq!(text(b"%%41"), Some("%A".to_string()), "the second `%` starts an escape");
}

#[tokio::test]
async fn url_text_refuses_bytes_that_are_not_utf8() {
    for encoded in [&b"%E9"[..], b"caf\xe9", b"%C3", b"%ff%fe"] {
        let refused = Rt::url_text(encoded).expect_err("not UTF-8 once decoded");
        let (status, json) = read(refused).await;
        assert_eq!(status, 200, "{encoded:?}: the envelope's status; `@http` gives it one");
        assert_eq!(code(&json), Some(INVALID), "{json}");
        assert_eq!(
            json.pointer("/errors/0/message").and_then(|m| m.as_str()),
            Some("a query or form parameter is not UTF-8 text"),
        );
    }
}

#[test]
fn url_pairs_splits_and_decodes() {
    let decoded = Rt::url_pairs(b"a=1&&b=&c&=x&d=1=2&e%3D=%26").ok();
    let expected = pairs(&[("a", "1"), ("b", ""), ("c", ""), ("", "x"), ("d", "1=2"), ("e=", "&")]);
    assert_eq!(decoded, Some(expected));
    assert_eq!(Rt::url_pairs(b"").ok(), Some(Vec::new()));
}

#[derive(Debug, PartialEq, Deserialize, Validate)]
struct Find {
    email: String,
    limit: Option<i64>,
    #[serde(default)]
    tags: Vec<String>,
}

fn find(query: &str) -> Option<Find> {
    let pairs = Rt::url_pairs(query.as_bytes()).ok()?;
    Rt::input_from_pairs(&pairs, FIND_FIELDS).ok()
}

#[test]
fn input_from_pairs_repeats_a_key_for_a_list() {
    let input = find("tags=a&email=e&tags=b&tags=a");
    let expected = Find {
        email: "e".into(),
        limit: None,
        tags: vec!["a".into(), "b".into(), "a".into()],
    };
    assert_eq!(input, Some(expected));
}

#[test]
fn input_from_pairs_leaves_an_absent_or_empty_value_to_its_default() {
    let input = find("email=e");
    assert_eq!(
        input,
        Some(Find {
            email: "e".into(),
            limit: None,
            tags: Vec::new()
        })
    );
    // A key with an empty value is a value, though: the empty string.
    let input = find("email=&tags=");
    assert_eq!(
        input,
        Some(Find {
            email: String::new(),
            limit: None,
            tags: vec![String::new()]
        })
    );
}

#[test]
fn input_from_pairs_takes_the_first_of_a_repeated_scalar_and_ignores_strangers() {
    let input = find("email=first&limit=1&email=second&limit=2&other=x");
    assert_eq!(
        input,
        Some(Find {
            email: "first".into(),
            limit: Some(1),
            tags: Vec::new()
        })
    );
}

#[tokio::test]
async fn a_missing_required_field_is_serde_s_message_without_a_parameter() {
    let pairs = Rt::url_pairs(b"limit=2").ok().unwrap_or_default();
    let refused = Rt::input_from_pairs::<Find>(&pairs, FIND_FIELDS).expect_err("email is required");
    let (status, json) = read(refused).await;
    assert_eq!(status, 200);
    assert_eq!(code(&json), Some(INVALID));
    let message = json
        .pointer("/errors/0/message")
        .and_then(|m| m.as_str())
        .unwrap_or_default();
    assert!(message.contains("missing field `email`"), "{json}");
    assert_eq!(json.pointer("/errors/0/source"), None, "{json}");
}

fn value(kind: QueryKind, raw: &str) -> Option<serde_json::Value> {
    let field = QueryField {
        name: "p",
        kind,
        list: false,
    };
    Rt::query_value(&field, raw).ok()
}

#[test]
fn query_value_converts_each_kind() {
    use serde_json::json;
    assert_eq!(value(QueryKind::Str, "2"), Some(json!("2")));
    assert_eq!(value(QueryKind::Int, "-2"), Some(json!(-2)));
    assert_eq!(value(QueryKind::Int, "9223372036854775807"), Some(json!(i64::MAX)));
    assert_eq!(value(QueryKind::Float, "1.5"), Some(json!(1.5)));
    assert_eq!(value(QueryKind::Float, "2"), Some(json!(2.0)));
    assert_eq!(value(QueryKind::Bool, "true"), Some(json!(true)));
    assert_eq!(value(QueryKind::Bool, "false"), Some(json!(false)));
    assert_eq!(
        value(QueryKind::Json, r#"{"k":[1,null]}"#),
        Some(json!({ "k": [1, null] }))
    );
    assert_eq!(value(QueryKind::Json, "null"), Some(json!(null)));
}

#[tokio::test]
async fn a_bad_value_is_refused_with_its_parameter_named() {
    let cases = [
        (QueryKind::Int, "1.0", "`1.0` is not a valid integer"),
        (
            QueryKind::Int,
            "9223372036854775808",
            "`9223372036854775808` is not a valid integer",
        ),
        (QueryKind::Float, "NaN", "`NaN` is not a valid number"),
        (QueryKind::Float, "x", "`x` is not a valid number"),
        (QueryKind::Bool, "TRUE", "`TRUE` is not a valid boolean"),
        (QueryKind::Bool, "1", "`1` is not a valid boolean"),
        (QueryKind::Json, "{", "`{` is not a valid JSON value"),
    ];
    for (kind, raw, message) in cases {
        let field = QueryField {
            name: "the_param",
            kind,
            list: false,
        };
        let refused = Rt::query_value(&field, raw).expect_err(raw);
        let (status, json) = read(refused).await;
        assert_eq!(status, 200);
        assert_eq!(code(&json), Some(INVALID), "{json}");
        assert_eq!(
            json.pointer("/errors/0/message").and_then(|m| m.as_str()),
            Some(message)
        );
        assert_eq!(
            json.pointer("/errors/0/source/parameter").and_then(|m| m.as_str()),
            Some("the_param"),
            "{json}",
        );
    }
}

#[tokio::test]
async fn a_bad_list_item_refuses_the_whole_input() {
    let fields = [QueryField {
        name: "ids",
        kind: QueryKind::Int,
        list: true,
    }];
    let pairs = Rt::url_pairs(b"ids=1&ids=x").ok().unwrap_or_default();
    let refused = Rt::input_from_pairs::<serde_json::Value>(&pairs, &fields).expect_err("x is no integer");
    let (_, json) = read(refused).await;
    assert_eq!(
        json.pointer("/errors/0/source/parameter").and_then(|m| m.as_str()),
        Some("ids")
    );
}

#[test]
fn query_kind_describes_itself_as_a_refusal_names_it() {
    let names = [
        QueryKind::Str,
        QueryKind::Int,
        QueryKind::Float,
        QueryKind::Bool,
        QueryKind::Json,
    ]
    .map(QueryKind::describe);
    assert_eq!(names, ["string", "integer", "number", "boolean", "JSON value"]);
}

#[test]
fn override_pairs_replaces_a_list_wholesale() {
    let mut query = pairs(&[("tags", "a"), ("email", "q"), ("tags", "b"), ("limit", "1")]);
    override_pairs(&mut query, pairs(&[("tags", "c"), ("email", "f")]));
    assert_eq!(query, pairs(&[("limit", "1"), ("tags", "c"), ("email", "f")]));

    let mut query = pairs(&[("a", "1")]);
    override_pairs(&mut query, Vec::new());
    assert_eq!(query, pairs(&[("a", "1")]), "nothing newer leaves the pairs alone");
}

#[derive(Debug, Deserialize, Validate)]
struct Bounded {
    #[validate(length(max = 3, message = "too long"))]
    name: String,
}

#[tokio::test]
async fn input_from_query_reads_and_query_input_validates() {
    let fields = [QueryField {
        name: "name",
        kind: QueryKind::Str,
        list: false,
    }];
    let unchecked: Option<Bounded> = Rt::input_from_query(Some("name=long+name"), &fields).ok();
    assert_eq!(unchecked.map(|input| input.name), Some("long name".to_string()));
    let absent = Rt::input_from_query::<Bounded>(None, &fields);
    assert!(absent.is_err(), "no query string is no `name`");

    let accepted: Option<Bounded> = Rt::query_input(Some("name=abc"), &fields).ok();
    assert_eq!(accepted.map(|input| input.name), Some("abc".to_string()));

    let refused = Rt::query_input::<Bounded>(Some("name=abcd"), &fields).expect_err("over the bound");
    let (status, json) = read(refused).await;
    assert_eq!(status, 200);
    assert_eq!(code(&json), Some(INVALID), "the rule's own code is rewritten: {json}");
    assert_eq!(
        json.pointer("/errors/0/message").and_then(|m| m.as_str()),
        Some("too long")
    );
    assert_eq!(
        json.pointer("/errors/0/source/pointer").and_then(|m| m.as_str()),
        Some("/name")
    );
}

/// An input whose fields serde types more narrowly than their query kinds: an
/// integer that must fit a `u8`, a string that must be a known variant, a JSON
/// value that must be an object of its own shape.
#[derive(Debug, Deserialize)]
#[allow(dead_code, reason = "only refusals are read")]
struct Narrow {
    level: Option<u8>,
    colour: Option<Colour>,
    filter: Option<Filter>,
    #[serde(default)]
    ids: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Colour {
    Red,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code, reason = "only refusals are read")]
struct Filter {
    min: i64,
}

static NARROW_FIELDS: &[QueryField] = &[
    QueryField::int("level"),
    QueryField::str("colour"),
    QueryField::json("filter"),
    QueryField::int("ids").list(),
];

#[tokio::test]
async fn a_value_serde_refuses_names_its_parameter() {
    let cases = [
        ("level=300", "level", "300"),
        ("colour=blue", "colour", "blue"),
        // Deeper than the parameter: still the parameter the caller wrote.
        (r#"filter={"min":"x"}"#, "filter", "invalid type"),
        ("ids=1&ids=300", "ids", "300"),
    ];
    for (query, parameter, reason) in cases {
        let refused = Rt::input_from_query::<Narrow>(Some(query), NARROW_FIELDS).expect_err(query);
        let (status, json) = read(refused).await;
        assert_eq!((status, code(&json)), (200, Some(INVALID)), "{query}: {json}");
        assert_eq!(
            json.pointer("/errors/0/source/parameter").and_then(|m| m.as_str()),
            Some(parameter),
            "{query}: {json}"
        );
        let message = json
            .pointer("/errors/0/message")
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        assert!(
            message.contains(reason),
            "{query}: serde's reason is the message: {json}"
        );
    }

    let accepted = Rt::input_from_query::<Narrow>(Some(r#"level=3&colour=red&filter={"min":1}&ids=4"#), NARROW_FIELDS);
    assert!(accepted.is_ok(), "the same fields in range read");
}
