//! Compile-only regression for derive-macro hygiene (F5).
//!
//! This crate depends on `leto` **without** a `use leto::prelude::*` glob
//! and **without** a direct `erris` dependency. The derive macros must therefore
//! reference every item by an absolute `leto::` path. If any generated code
//! uses a bare `ApiResponse` or `erris`, this crate fails to compile — which is
//! exactly the regression we guard against.
#![allow(dead_code)]

use leto::{ApiError, ApiErrorCode, ErrorMessage, FromErrorMessage};
use thiserror::Error;

// Exercises the generated `From<_> for Result<leto::ApiResponse<T, M>, _>`
// impl, which previously referenced a bare `ApiResponse`.
#[derive(Clone, Debug, ApiErrorCode)]
enum OrderCode {
    #[message("order {id} not found")]
    NotFound { id: u64 },
    #[code("order.already_paid")]
    AlreadyPaid,
}

// Exercises the generated `From<leto::erris::Report>` catch-all impl, which
// previously referenced a bare `erris`. The field type is written the way the
// docs instruct — `leto::erris::Report` — with no direct `erris` dependency.
#[derive(Debug, Error, ApiError)]
enum ServiceError {
    #[error("forbidden")]
    #[code("forbidden")]
    Forbidden,

    #[catch_all]
    #[error("internal")]
    #[code("internal")]
    Internal(#[source] leto::erris::Report),
}

// Exercises the FromErrorMessage generated arms without the prelude in scope.
#[derive(Debug, FromErrorMessage)]
enum ClientError {
    #[code("not_found")]
    NotFound(ErrorMessage),
    #[code("_")]
    Other(ErrorMessage),
}

// Reference each generated impl so the trait bounds are actually checked.
fn _assert_impls() {
    let _: ApiError<OrderCode> = OrderCode::AlreadyPaid.into();
    let _: Result<leto::ApiResponse<(), ()>, OrderCode> = OrderCode::AlreadyPaid.into();
    let _: ApiError = ServiceError::Forbidden.into();
    let _: ServiceError = leto::erris::report!("boom").into();
    let _: ClientError = leto::error("not_found").into();
}

/// The generated code must resolve the runtime crate by the name the *dependant*
/// uses, not a hardcoded `leto::`. This pins the core-only dependency graph (the
/// renamed-facade case lives in `leto-hygiene-renamed`, since cargo forbids
/// depending on one path crate under two names). Both failed to compile before
/// the macros switched to `proc-macro-crate` resolution.
mod crate_name_hygiene {
    // Core-only: the facade is not a dependency of this module's graph at all,
    // so the macros must fall back to `leto_core`.
    mod core_only {
        #[derive(Clone, Debug, leto_derive::ApiErrorCode)]
        enum Code {
            #[message("order {id} not found")]
            NotFound { id: u64 },
            #[code("order.already_paid")]
            AlreadyPaid,
        }

        #[derive(Debug, leto_derive::FromErrorMessage)]
        enum Client {
            #[code("not_found")]
            NotFound(leto_core::ErrorMessage),
            #[code("_")]
            Other(leto_core::ErrorMessage),
        }

        fn _assert_impls() {
            let _: leto_core::ApiError<Code> = Code::NotFound { id: 1 }.into();
            let _: leto_core::ApiError<Code> = Code::AlreadyPaid.into();
            let _: Client = leto_core::error("not_found").into();
        }
    }
}
