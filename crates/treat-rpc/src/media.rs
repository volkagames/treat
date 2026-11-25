//! Media types as both halves of the runtime compare them: by the type and
//! subtype alone (the part before any `;` parameter), without regard to case,
//! as RFC 9110 defines them.
//!
//! One set of helpers so the server, deciding what it reads, and the client,
//! deciding what it was answered with, cannot disagree about whether a
//! `Content-Type` names a type.

/// The media type of a `Content-Type` value: what comes before any `;`
/// parameter, without surrounding whitespace.
pub(crate) fn essence(content_type: &str) -> &str {
    content_type.split(';').next().unwrap_or_default().trim()
}

/// Whether a `Content-Type` value is `expected`: its media type alone,
/// compared without regard to case. Exact, so `application/jsonl` is not
/// `application/json`, nor `application/x-www-form-urlencoded-x` a form.
pub(crate) fn is(content_type: &str, expected: &str) -> bool {
    essence(content_type).eq_ignore_ascii_case(expected)
}

/// Whether a `Content-Type` value declares JSON: `application/json`, or any
/// type with the `+json` structured-syntax suffix (RFC 6839) such as
/// `application/problem+json`, in any case and with any parameters. The
/// suffix needs a name before it: `application/+json` is no JSON type.
#[cfg(feature = "server")]
pub(crate) fn is_json(content_type: &str) -> bool {
    let Some((kind, subtype)) = essence(content_type).split_once('/') else {
        return false;
    };
    let suffixed = subtype
        .len()
        .checked_sub(JSON_SUFFIX.len())
        // A name before the suffix: `+json` alone is no subtype.
        .filter(|start| *start > 0)
        .and_then(|start| subtype.get(start..))
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(JSON_SUFFIX));
    let plain = kind.eq_ignore_ascii_case("application") && subtype.eq_ignore_ascii_case("json");
    !kind.is_empty() && (plain || suffixed)
}

/// The structured-syntax suffix of a JSON media type.
#[cfg(feature = "server")]
const JSON_SUFFIX: &str = "+json";

/// Whether a `Content-Type` value is one an operation declares it accepts.
///
/// `declared` is a media range as an `Accept` header or an OpenAPI request
/// body spells one: `text/csv` takes that type alone, `image/*` any subtype of
/// `image`, and `*/*` any type at all. Both sides are compared by media type
/// alone, without regard to case; a parameter on either is not compared. A
/// value with no `type/subtype` shape — `csv`, `*/csv`, the empty string — is
/// no media type, and matches nothing.
#[cfg(feature = "server")]
pub(crate) fn matches_range(content_type: &str, declared: &str) -> bool {
    let Some((kind, subtype)) = essence(content_type).split_once('/') else {
        return false;
    };
    let Some((range_kind, range_subtype)) = essence(declared).split_once('/') else {
        return false;
    };
    if kind.is_empty() || subtype.is_empty() || kind == "*" || subtype == "*" {
        return false;
    }
    match (range_kind, range_subtype) {
        ("*", "*") => true,
        ("*", _) => false,
        (range_kind, "*") => range_kind.eq_ignore_ascii_case(kind),
        (range_kind, range_subtype) => {
            range_kind.eq_ignore_ascii_case(kind) && range_subtype.eq_ignore_ascii_case(subtype)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_media_type_is_compared_exactly_without_its_parameters() {
        assert!(is("application/json", "application/json"));
        assert!(is("Application/JSON; charset=utf-8", "application/json"));
        assert!(is(" text/event-stream ;x=1", "text/event-stream"));
        assert!(is(
            "Application/X-WWW-Form-Urlencoded; charset=UTF-8",
            "application/x-www-form-urlencoded"
        ));
        assert!(!is("application/jsonl", "application/json"));
        assert!(!is("application/json-seq", "application/json"));
        assert!(!is(
            "application/x-www-form-urlencodedx",
            "application/x-www-form-urlencoded"
        ));
        assert!(!is("text/html", "text/event-stream"));
        assert!(!is("", "application/json"));
    }

    #[cfg(feature = "server")]
    #[test]
    fn json_is_application_json_or_a_json_suffix() {
        for json in [
            "application/json",
            "Application/JSON; charset=utf-8",
            "application/vnd.api+json",
            "application/problem+JSON",
        ] {
            assert!(is_json(json), "{json}");
        }
        for other in [
            "application/jsonl",
            "json",
            "+json",
            "/+json",
            "application/+json",
            "application/+JSON; charset=utf-8",
            "text/plain",
            "",
        ] {
            assert!(!is_json(other), "{other}");
        }
    }

    #[cfg(feature = "server")]
    #[test]
    fn a_declared_range_takes_its_type_or_every_subtype_of_it() {
        assert!(matches_range("text/csv", "text/csv"));
        assert!(matches_range("Text/CSV; charset=utf-8", "text/csv"));
        assert!(matches_range("text/csv", "TEXT/CSV; header=present"));
        assert!(matches_range("image/png", "image/*"));
        assert!(matches_range("IMAGE/png", "image/*"));
        assert!(matches_range("application/octet-stream", "*/*"));

        assert!(!matches_range("text/plain", "text/csv"));
        assert!(!matches_range("text/csvx", "text/csv"));
        assert!(!matches_range("text/png", "image/*"));
        assert!(!matches_range("text/csv", "*/csv"), "not a media range");
        assert!(!matches_range("text/csv", "csv"));
        assert!(!matches_range("text/csv", ""));
        assert!(!matches_range("", "*/*"), "no type is not any type");
        assert!(!matches_range("csv", "*/*"));
        assert!(!matches_range("image/*", "image/png"), "a range is not a type");
        assert!(!matches_range("*/*", "*/*"));
    }
}
