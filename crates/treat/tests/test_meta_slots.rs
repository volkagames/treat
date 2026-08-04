//! Coverage for the typed `meta` slots (`Pagination`, `RateLimit`) and the
//! `success_with_meta` constructor that attaches them.
#![cfg(feature = "meta-slots")]

use treat::meta_slots::{Pagination, RateLimit};
use treat::success_with_meta;
use serde_json::json;

#[test]
fn pagination_computes_total_pages() {
    assert_eq!(Pagination::new(2, 20, 137).total_pages, 7); // ceil(137 / 20)
    assert_eq!(Pagination::new(1, 20, 100).total_pages, 5); // exact multiple
    assert_eq!(Pagination::new(1, 20, 0).total_pages, 0); // empty
    assert_eq!(Pagination::new(1, 0, 5).total_pages, 0); // guard against div by zero
}

#[test]
fn pagination_serializes() {
    let value = serde_json::to_value(Pagination::new(2, 20, 137)).expect("serialize");
    assert_eq!(
        value,
        json!({ "page": 2, "per_page": 20, "total": 137, "total_pages": 7 })
    );
}

#[test]
fn rate_limit_serializes() {
    let value = serde_json::to_value(RateLimit::new(100, 42, 1_700_000_000)).expect("serialize");
    assert_eq!(
        value,
        json!({ "limit": 100, "remaining": 42, "reset": 1_700_000_000_u64 })
    );
}

#[test]
fn success_with_meta_wraps_data_and_meta() {
    let resp = success_with_meta(vec![1, 2, 3], Pagination::new(1, 3, 9));
    let value = serde_json::to_value(&resp).expect("serialize");
    assert_eq!(
        value,
        json!({
            "data": [1, 2, 3],
            "meta": { "page": 1, "per_page": 3, "total": 9, "total_pages": 3 },
        })
    );
}
