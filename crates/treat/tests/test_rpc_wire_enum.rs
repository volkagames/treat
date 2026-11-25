//! `#[derive(treat::rpc::WireEnum)]`: where a wire name comes from, and that
//! `as_str`, `Display` and serde agree on it where the names are spelled.
//!
//! The derive's compile errors (`#[code]` disagreeing with serde, a duplicate
//! wire name, a second `#[undeclared]`, a variant with fields) are not covered
//! here: the crate has no compile-fail harness (`trybuild`) among its
//! dev-dependencies.
#![cfg(feature = "derive")]
#![allow(non_camel_case_types)]

use serde::{Deserialize, Serialize};
use treat::prelude::*;
use treat::rpc::{Undeclared, WireEnum};

/// The shape a generated flat registry has: snake_case identifiers, a tagged
/// code whose wire name is not an identifier, `#[code]` and serde spelling the
/// same string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ApiErrorCode, treat::rpc::WireEnum)]
enum ErrorCode {
    #[message("invalid body")]
    invalid_body,
    #[message("internal error")]
    internal_error,
    #[message("not found")]
    not_found,
    #[message("gone")]
    #[code("things::gone")]
    #[serde(rename = "things::gone")]
    things__gone,
}

#[test]
fn wire_name_is_code_then_rename_then_ident() {
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, treat::rpc::WireEnum)]
    enum Mixed {
        #[code("from_code")]
        Code,
        #[serde(rename = "from_rename")]
        Rename,
        // `#[code]` wins over the identifier even with serde silent, and agrees
        // with an equal rename.
        #[code("both")]
        #[serde(rename = "both")]
        Both,
        Ident,
    }

    assert_eq!(Mixed::Code.as_str(), "from_code");
    assert_eq!(Mixed::Rename.as_str(), "from_rename");
    assert_eq!(Mixed::Both.as_str(), "both");
    assert_eq!(Mixed::Ident.as_str(), "Ident");
    assert_eq!(Mixed::from_name("from_rename"), Some(Mixed::Rename));
    assert_eq!(Mixed::from_name("Rename"), None);
}

/// `as_str` is the wire name, not serde's: a `#[code]` serde is not told
/// leaves serde writing the identifier, and the derive accepts that.
#[test]
fn a_code_serde_is_not_told_is_the_wire_name_but_not_serde_s() {
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, treat::rpc::WireEnum)]
    enum Tagged {
        #[code("things::gone")]
        ThingsGone,
    }

    assert_eq!(Tagged::ThingsGone.as_str(), "things::gone");
    assert_eq!(
        serde_json::to_string(&Tagged::ThingsGone).expect("serialize"),
        "\"ThingsGone\""
    );
}

#[test]
fn rename_all_is_what_serde_writes() {
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, treat::rpc::WireEnum)]
    #[serde(rename_all = "snake_case")]
    enum Status {
        NotFound,
        #[serde(rename = "gone")]
        AlreadyGone,
        #[code("internal_error")]
        InternalError,
    }

    for status in Status::ALL {
        let json = serde_json::to_string(status).expect("serialize");
        assert_eq!(json, format!("\"{}\"", status.as_str()));
        let back: Status = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, *status);
    }
    assert_eq!(Status::NotFound.as_str(), "not_found");
}

#[test]
fn tagged_code_round_trips_through_every_writer() {
    let gone = ErrorCode::things__gone;
    assert_eq!(gone.as_str(), "things::gone");
    assert_eq!(gone.to_string(), "things::gone");
    assert_eq!(serde_json::to_string(&gone).expect("serialize"), "\"things::gone\"");
    assert_eq!(ErrorCode::from_name("things::gone"), Some(gone));
    assert_eq!(ErrorCode::from_name("things__gone"), None);
}

#[test]
fn all_lists_every_variant_in_declaration_order() {
    assert_eq!(
        ErrorCode::ALL,
        &[
            ErrorCode::invalid_body,
            ErrorCode::internal_error,
            ErrorCode::not_found,
            ErrorCode::things__gone,
        ],
    );
    for code in ErrorCode::ALL {
        assert_eq!(ErrorCode::from_name(code.as_str()), Some(*code));
        assert_eq!(code.to_string(), code.as_str());
    }
}

#[test]
fn undeclared_marks_the_fallback_variant() {
    #[derive(Debug, Clone, Copy, PartialEq, ApiErrorCode, treat::rpc::WireEnum)]
    enum GetCodes {
        #[code("things::gone")]
        ThingsGone,
        #[code("internal_error")]
        #[undeclared]
        InternalError,
    }

    assert_eq!(GetCodes::UNDECLARED, GetCodes::InternalError);
    assert_eq!(GetCodes::ThingsGone.to_string(), GetCodes::ThingsGone.as_str());
}

#[test]
fn empty_enum_has_no_names() {
    #[derive(Debug, Clone, Copy, treat::rpc::WireEnum)]
    enum Nothing {}

    assert!(Nothing::ALL.is_empty());
    assert!(Nothing::from_name("").is_none());
}

/// Asserts that every value's `as_str` is exactly what serde writes for it and
/// reads back, and that `from_name` inverts `as_str`.
fn assert_serde_agrees<E>(all: &[E])
where
    E: WireEnum + Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    for value in all {
        let json = serde_json::to_string(value).expect("serialize");
        assert_eq!(json, format!("\"{}\"", value.as_str()), "{value:?}");
        let back: E = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, *value);
        assert_eq!(E::from_name(value.as_str()), Some(*value));
    }
}

#[test]
fn raw_identifier_is_unprefixed_everywhere() {
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, ApiErrorCode, treat::rpc::WireEnum)]
    enum Keyword {
        r#type,
        r#match,
        plain,
    }

    assert_eq!(Keyword::r#type.as_str(), "type");
    assert_eq!(Keyword::from_name("type"), Some(Keyword::r#type));
    assert_eq!(Keyword::from_name("r#type"), None);
    for keyword in Keyword::ALL {
        // `ApiErrorCode`'s `Display` agrees with `WireEnum` without a `#[code]`.
        assert_eq!(keyword.to_string(), keyword.as_str());
    }
    assert_serde_agrees(Keyword::ALL);
}

/// Before 0.26 `ApiErrorCode` sent a raw identifier with its prefix; a
/// `#[code]` keeps that code for clients that still match it.
#[test]
fn a_code_keeps_the_prefixed_spelling_of_a_raw_identifier() {
    #[derive(Debug, Clone, ApiErrorCode)]
    enum Legacy {
        #[code("r#type")]
        r#type,
        r#match,
    }

    assert_eq!(Legacy::r#type.to_string(), "r#type");
    assert_eq!(Legacy::r#match.to_string(), "match");
}

#[test]
fn raw_identifier_goes_through_rename_all_unprefixed() {
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, treat::rpc::WireEnum)]
    #[serde(rename_all = "UPPERCASE")]
    enum Keyword {
        r#type,
    }

    assert_eq!(Keyword::r#type.as_str(), "TYPE");
    assert_serde_agrees(Keyword::ALL);
}

#[test]
fn as_str_takes_the_value_and_works_through_a_reference() {
    let by_ref: &ErrorCode = &ErrorCode::things__gone;
    assert_eq!(by_ref.as_str(), "things::gone");
    assert_eq!(ErrorCode::not_found.as_str(), "not_found");
}

#[test]
fn rename_halves_naming_one_string_are_the_wire_name() {
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, treat::rpc::WireEnum)]
    enum Halves {
        #[serde(rename(serialize = "both_ways", deserialize = "both_ways"))]
        BothWays,
        #[code("tagged")]
        #[serde(rename(deserialize = "tagged", serialize = "tagged"))]
        Tagged,
        // One half naming the identifier: the other half writes it anyway.
        #[serde(rename(serialize = "Lone"))]
        Lone,
    }

    assert_eq!(Halves::BothWays.as_str(), "both_ways");
    assert_eq!(Halves::Tagged.as_str(), "tagged");
    assert_eq!(Halves::Lone.as_str(), "Lone");
    assert_serde_agrees(Halves::ALL);
}

/// `rename_all` halves that name different rules are fine for a variant that
/// spells its own name, and for one both rules spell the same.
#[test]
fn differing_rename_all_halves_are_fine_where_no_variant_gets_two_names() {
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, treat::rpc::WireEnum)]
    #[serde(rename_all(serialize = "snake_case", deserialize = "kebab-case"))]
    enum Split {
        #[serde(rename = "not_found")]
        NotFound,
        #[code("gone_away")]
        #[serde(rename = "gone_away")]
        GoneAway,
        // `snake_case` and `kebab-case` both write a one-word name as `single`.
        Single,
    }

    assert_eq!(Split::NotFound.as_str(), "not_found");
    assert_eq!(Split::GoneAway.as_str(), "gone_away");
    assert_eq!(Split::Single.as_str(), "single");
    assert_serde_agrees(Split::ALL);
}

/// Every serde `rename_all` rule, checked against what serde itself writes and
/// reads rather than against a hand-written expectation.
macro_rules! rename_all_rules {
    ($($test:ident => $rule:literal),* $(,)?) => {$(
        #[test]
        fn $test() {
            #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, treat::rpc::WireEnum)]
            #[serde(rename_all = $rule)]
            enum Rule {
                NotFound,
                A,
                HttpError2,
                ThreeWordName,
                r#type,
                lower_snake,
            }

            assert_serde_agrees(Rule::ALL);
        }
    )*};
}

rename_all_rules! {
    rename_all_lowercase => "lowercase",
    rename_all_uppercase => "UPPERCASE",
    rename_all_pascal_case => "PascalCase",
    rename_all_camel_case => "camelCase",
    rename_all_snake_case => "snake_case",
    rename_all_screaming_snake_case => "SCREAMING_SNAKE_CASE",
    rename_all_kebab_case => "kebab-case",
    rename_all_screaming_kebab_case => "SCREAMING-KEBAB-CASE",
}

/// serde keys other than `rename` / `rename_all` are skipped, on the enum and
/// on a variant, and none of them is mirrored: an alias is no wire name, and
/// `ALL` keeps an `other` or skipped variant.
#[test]
fn other_serde_keys_are_skipped_and_not_mirrored() {
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, treat::rpc::WireEnum)]
    #[serde(rename = "Container", rename_all = "snake_case", deny_unknown_fields)]
    enum Keys {
        #[serde(alias = "nf", rename = "not_found", alias = "missing")]
        NotFound,
        #[serde(skip_serializing, rename = "hidden")]
        Hidden,
        #[serde(other)]
        Unknown,
    }

    assert_eq!(Keys::NotFound.as_str(), "not_found");
    assert_eq!(Keys::from_name("nf"), None);
    assert_eq!(Keys::Hidden.as_str(), "hidden");
    assert_eq!(Keys::Unknown.as_str(), "unknown");
    assert_eq!(Keys::ALL, &[Keys::NotFound, Keys::Hidden, Keys::Unknown]);
    // serde reads the alias, which `from_name` does not.
    let aliased: Keys = serde_json::from_str("\"nf\"").expect("alias");
    assert_eq!(aliased, Keys::NotFound);
}

/// An enum with only `WireEnum`, used through `treat-core`'s path: the facade's
/// trait is the core's, so code written against `treat_core::rpc` takes the
/// derive's impl as is.
#[test]
fn derive_implements_the_core_trait() {
    #[derive(Debug, Clone, Copy, PartialEq, treat::rpc::WireEnum)]
    enum CoreOnly {
        #[code("first")]
        First,
        Second,
    }

    fn names<E: treat_core::rpc::WireEnum>() -> Vec<&'static str> {
        E::ALL.iter().map(|value| value.as_str()).collect()
    }

    assert_eq!(names::<CoreOnly>(), ["first", "Second"]);
}
