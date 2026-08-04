//! Compile-only regression for derive-macro hygiene (F5).
//!
//! This crate depends on `treat` **without** a `use treat::prelude::*` glob
//! and **without** a direct `erris` dependency. The derive macros must therefore
//! reference every item by an absolute `treat::` path. If any generated code
//! uses a bare `ApiResponse` or `erris`, this crate fails to compile — which is
//! exactly the regression we guard against.
#![allow(dead_code)]

use thiserror::Error;
use treat::{ApiError, ApiErrorCode, ErrorMessage, FromErrorMessage};

// Exercises the generated `From<_> for Result<treat::ApiResponse<T, M>, _>`
// impl, which previously referenced a bare `ApiResponse`.
#[derive(Clone, Debug, ApiErrorCode)]
enum OrderCode {
    #[message("order {id} not found")]
    NotFound { id: u64 },
    #[code("order.already_paid")]
    AlreadyPaid,
}

// Exercises the generated `From<treat::erris::Report>` catch-all impl, which
// previously referenced a bare `erris`. The field type is written the way the
// docs instruct — `treat::erris::Report` — with no direct `erris` dependency.
#[derive(Debug, Error, ApiError)]
enum ServiceError {
    #[error("forbidden")]
    #[code("forbidden")]
    Forbidden,

    #[catch_all]
    #[error("internal")]
    #[code("internal")]
    Internal(#[source] treat::erris::Report),
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
    let _: Result<treat::ApiResponse<(), ()>, OrderCode> = OrderCode::AlreadyPaid.into();
    let _: ApiError = ServiceError::Forbidden.into();
    let _: ServiceError = treat::erris::report!("boom").into();
    let _: ClientError = treat::error("not_found").into();
}

/// The generated code must resolve the runtime crate by the name the *dependant*
/// uses, not a hardcoded `treat::`. This pins the core-only dependency graph (the
/// renamed-facade case lives in `treat-hygiene-renamed`, since cargo forbids
/// depending on one path crate under two names). Both failed to compile before
/// the macros switched to `proc-macro-crate` resolution.
mod crate_name_hygiene {
    // Core-only: the facade is not a dependency of this module's graph at all,
    // so the macros must fall back to `treat_core`.
    mod core_only {
        #[derive(Clone, Debug, treat_derive::ApiErrorCode)]
        enum Code {
            #[message("order {id} not found")]
            NotFound { id: u64 },
            #[code("order.already_paid")]
            AlreadyPaid,
        }

        #[derive(Debug, treat_derive::FromErrorMessage)]
        enum Client {
            #[code("not_found")]
            NotFound(treat_core::ErrorMessage),
            #[code("_")]
            Other(treat_core::ErrorMessage),
        }

        fn _assert_impls() {
            let _: treat_core::ApiError<Code> = Code::NotFound { id: 1 }.into();
            let _: treat_core::ApiError<Code> = Code::AlreadyPaid.into();
            let _: Client = treat_core::error("not_found").into();
        }
    }
}
