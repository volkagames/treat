//! Checks generated validation code calls for rules `validator` has no
//! attribute for. Enabled by the `validator` feature.

/// The `regex` crate the patterns are compiled with.
///
/// [`matches_pattern`] takes `treat`'s `Regex`, so generated code compiles its
/// patterns through this re-export rather than a `regex` dependency of its own
/// that would have to match `treat`'s version.
pub use regex;
use regex::Regex;

/// Whether a value satisfies a pattern, given the regex compiled for it.
///
/// `None` is a pattern that did not compile, and rejects. A generator compiles
/// every pattern before emitting it, so that is a generator bug rather than a
/// runtime condition — and rejecting is the safe side of one, because the
/// alternative accepts input no rule ever checked.
///
/// Takes `&Option<Regex>` so a `static PATTERN: LazyLock<Option<Regex>>` is
/// passed as `&PATTERN`: the pattern compiles once, on first use.
pub fn matches_pattern<T>(pattern: &Option<Regex>, value: &T) -> bool
where
    T: validator::ValidateRegex + ?Sized,
{
    pattern
        .as_ref()
        .is_some_and(|regex| validator::ValidateRegex::validate_regex(value, regex))
}
