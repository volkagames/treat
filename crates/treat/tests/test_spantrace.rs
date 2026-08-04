//! Coverage for spantrace capture on `ApiError` (feature-gated).
#![cfg(feature = "spantrace")]

use treat::error;

#[test]
fn spantrace_accessor_is_available_within_a_span() {
    let span = tracing::info_span!("root_span");
    let e = span.in_scope(|| error("boom"));

    // The accessor exists under the `spantrace` feature and never panics.
    let _ = e.spantrace().to_string();

    // The non-alternate Debug rendering includes the error itself.
    let dbg = format!("{e:?}");
    assert!(dbg.contains("boom"), "debug output missing code: {dbg}");
}
