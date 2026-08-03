// The only nightly feature left: enabling `Error::provide` for extracting the
// source report / backtrace / spantrace from a `&dyn Error`. Off by default so
// the crate builds on stable.
#![cfg_attr(feature = "nightly-provide", feature(error_generic_member_access))]

//! Core of the `leto` response envelope.
//!
//! - [`ApiResponse`] — the `{ data, meta, errors[] }` envelope.
//! - [`ApiError`] — a typed error (`code`, `message`, `meta`, source chain via
//!   `erris`, `#[track_caller]` location, optional spantrace/backtrace).
//! - Ergonomic conversions: [`OkOrError`], [`WrapApiError`], [`ApiErrorTrack`].
//!
//! Framework adapters live behind the `actix` / `axum` / `poem` features. Most users
//! depend on the `leto` facade crate, not this crate directly. Builds on
//! stable Rust; the `nightly-provide` feature adds `Error::provide` support.

/// Re-export of the [`erris`] error-reporting crate that backs `ApiError`'s
/// source chain. Use `leto::erris::report!`, `erris::Report`, `erris::Result`
/// directly instead of adding `erris` to your own `Cargo.toml`.
pub use erris;

pub mod error;
pub mod error_message;
pub mod error_report;
#[cfg(feature = "meta-slots")]
pub mod meta_slots;
pub mod ok_or_error;
pub mod response;
pub mod response_report;
#[cfg(feature = "rpc-status-header")]
pub mod rpc_status;
#[cfg(feature = "serde-path")]
pub mod serde_path;
pub mod track;
#[cfg(feature = "validator")]
pub mod validate;
pub mod wrap_error_code;

pub use error::*;
pub use error_message::*;
pub use error_report::*;
pub use ok_or_error::*;
pub use response::*;
#[cfg(feature = "serde-path")]
pub use serde_path::*;
pub use track::*;
#[cfg(feature = "validator")]
pub use validate::*;
pub use wrap_error_code::*;

pub mod prelude {
    pub use super::ok_or_error::*;
    pub use super::response::*;
    pub use super::wrap_error_code::*;
    pub use super::{ApiError, ApiErrorCode, ApiErrorTrack, ApiResponse, ErrorMessage};
}

cfg_block::cfg_block! {
    #[cfg(feature = "axum")] {
        pub mod error_axum;
        pub mod response_axum;

        pub use error_axum::*;
    }

    #[cfg(feature = "actix")] {
        pub mod error_actix_web;
        pub mod response_actix_web;

        pub use error_actix_web::response_get_api_error_actix;
    }

    #[cfg(feature = "poem")] {
        pub mod error_poem;
        pub mod response_poem;

        pub use error_poem::response_get_api_error_poem;
    }
}

// Body extractors that report parse/validation failures in the envelope.
// They need `serde-path` (the locating deserializer) plus a framework; each
// framework has its own `ApiJson`/`ApiValidated`, so they are not re-exported
// with a glob (that would collide) — reach them via `leto::extract_axum` /
// `leto::extract_actix`.
#[cfg(all(feature = "serde-path", feature = "actix"))]
pub mod extract_actix;
#[cfg(all(feature = "serde-path", feature = "axum"))]
pub mod extract_axum;
