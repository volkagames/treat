//! Ready-made `meta` payloads for the common cases, usable as the `M` type of
//! [`ApiResponse`](crate::ApiResponse):
//!
//! ```
//! use treat_core::{success_with_meta, meta_slots::Pagination};
//!
//! let page = Pagination::new(2, 20, 137); // page 2, 20 per page, 137 total
//! let resp = success_with_meta(vec![1, 2, 3], page);
//! assert_eq!(resp.meta.expect("meta").total_pages, 7);
//! ```

use serde::{Deserialize, Serialize};

/// Pagination metadata. `total_pages` is derived from `total` and `per_page` by
/// [`Pagination::new`]; construct it directly if you need custom values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Pagination {
    /// 1-based index of the current page.
    pub page: u64,
    /// Number of items requested per page.
    pub per_page: u64,
    /// Total number of items across all pages.
    pub total: u64,
    /// Total number of pages, `ceil(total / per_page)`.
    pub total_pages: u64,
}

impl Pagination {
    /// Build pagination metadata, computing `total_pages` as `ceil(total / per_page)`
    /// (`0` when `per_page` is `0`).
    pub fn new(page: u64, per_page: u64, total: u64) -> Self {
        let total_pages = match per_page {
            0 => 0,
            n => total.div_ceil(n),
        };
        Self {
            page,
            per_page,
            total,
            total_pages,
        }
    }
}

/// Rate-limit metadata, mirroring the `RateLimit-*` response headers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RateLimit {
    /// Maximum number of requests allowed in the window.
    pub limit: u64,
    /// Requests remaining in the current window.
    pub remaining: u64,
    /// Unix timestamp (seconds) when the window resets.
    pub reset: u64,
}

impl RateLimit {
    pub fn new(limit: u64, remaining: u64, reset: u64) -> Self {
        Self {
            limit,
            remaining,
            reset,
        }
    }
}
