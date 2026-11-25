//! `treat::rpc::validate::matches_pattern`: a pattern compiled once, and one
//! that did not compile rejecting.
#![cfg(feature = "validator")]

use std::sync::LazyLock;
use treat::rpc::validate::matches_pattern;
use treat::rpc::validate::regex::Regex;

static SLUG: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new("^[a-z]+(-[a-z]+)*$").ok());
// `black_box` hides the literal from clippy, which rejects an invalid regex it can see.
static BROKEN: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(std::hint::black_box("(")).ok());

#[test]
fn compiled_pattern_checks_the_value() {
    assert!(matches_pattern(&SLUG, "hello-world"));
    assert!(!matches_pattern(&SLUG, "Hello World"));
    assert!(matches_pattern(&SLUG, &"owned".to_string()));
}

#[test]
fn pattern_that_did_not_compile_rejects() {
    assert!(BROKEN.is_none());
    assert!(!matches_pattern(&BROKEN, "anything"));
}
