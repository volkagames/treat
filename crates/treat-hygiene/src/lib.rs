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

        #[derive(Clone, Copy, treat_derive::WireEnum)]
        enum Wire {
            #[code("not_found")]
            NotFound,
            #[undeclared]
            InternalError,
        }

        fn _assert_wire_impls() {
            let _: Option<Wire> = <Wire as treat_core::rpc::WireEnum>::from_name("not_found");
            let _: Wire = <Wire as treat_core::rpc::Undeclared>::UNDECLARED;
        }
    }
}

// Exercises the `WireEnum` derive without the prelude in scope: the generated
// impls must name `rpc::WireEnum` / `rpc::Undeclared` through the resolved
// runtime path, and `Option` through `::core`.
#[derive(Clone, Copy, Debug, treat::rpc::WireEnum)]
enum WireCode {
    #[code("things::gone")]
    ThingsGone,
    #[code("internal_error")]
    #[undeclared]
    InternalError,
}

fn _assert_wire_impls() {
    use treat::rpc::{Undeclared, WireEnum};
    let _: Option<WireCode> = WireCode::from_name("things::gone");
    let _: &'static str = WireCode::ThingsGone.as_str();
    let _: &'static [WireCode] = WireCode::ALL;
    let _: WireCode = WireCode::UNDECLARED;
}

/// Every derive expanded next to local items that shadow the std prelude, the
/// primitive types the generated signatures name, the `format!` / `write!`
/// macros and the `std` crate itself. The generated code must reach each of
/// them through an absolute `::core` / `::std` path, so none of these local
/// names can capture it.
#[allow(non_camel_case_types)]
mod shadowed_names {
    use treat::{ApiErrorCode, ErrorMessage, FromErrorMessage};

    struct From;
    struct Into;
    struct ToString;
    struct Option;
    struct Some;
    struct None;
    struct Result;
    struct Ok;
    struct Err;
    struct String;
    struct Vec;
    struct Box;
    struct Default;
    struct Display;
    struct Formatter;
    struct u16;
    struct str;
    mod std {}
    mod fmt {}
    #[allow(unused_macros)]
    macro_rules! format {
        ($($tt:tt)*) => {
            compile_error!("generated code must not call a local `format!`")
        };
    }
    #[allow(unused_macros)]
    macro_rules! write {
        ($($tt:tt)*) => {
            compile_error!("generated code must not call a local `write!`")
        };
    }

    #[derive(Clone, Debug, ApiErrorCode)]
    enum OrderCode {
        // A field named like the conversion's own argument: the generated
        // `From` must not let the variant's binding capture it.
        #[message("order {id} not found ({value})")]
        #[status(404)]
        NotFound { id: u64, value: u64 },
        #[message("bad {arg_0}")]
        Bad(u64),
        #[code("order.already_paid")]
        AlreadyPaid,
    }

    #[derive(Debug, thiserror::Error, treat::ApiError)]
    enum ServiceError {
        #[error("forbidden")]
        #[code("forbidden")]
        #[status(403)]
        Forbidden,

        #[catch_all]
        #[error("internal")]
        #[code("internal")]
        Internal(#[source] treat::erris::Report),
    }

    #[derive(Debug, FromErrorMessage)]
    enum ClientError {
        #[code("not_found")]
        NotFound(ErrorMessage),
        #[code("_")]
        Other { message: ErrorMessage },
    }

    #[derive(Clone, Copy, Debug, treat::rpc::WireEnum)]
    enum WireCode {
        #[code("things::gone")]
        ThingsGone,
        #[undeclared]
        InternalError,
    }

    // An enum named like the generic parameters a generated impl declares.
    #[derive(Clone, Debug, ApiErrorCode)]
    enum T {
        Gone,
    }

    #[derive(Clone, Debug, ApiErrorCode)]
    enum M {
        Gone,
    }

    fn _assert_impls() {
        use treat::rpc::{Undeclared, WireEnum};
        let _: treat::ApiError<OrderCode> = ::core::convert::Into::into(OrderCode::NotFound { id: 1, value: 2 });
        let _: treat::ApiError<OrderCode> = ::core::convert::Into::into(OrderCode::Bad(3));
        let _: ::core::result::Result<treat::ApiResponse<(), ()>, OrderCode> =
            ::core::convert::Into::into(OrderCode::AlreadyPaid);
        let _: ::core::primitive::u16 = treat::ApiErrorStatus::status_code(&OrderCode::AlreadyPaid);
        let _: treat::ApiError = ::core::convert::Into::into(ServiceError::Forbidden);
        let _: ServiceError = ::core::convert::Into::into(treat::erris::report!("boom"));
        let _: ClientError = ::core::convert::Into::into(treat::error("not_found"));
        let _: ::core::option::Option<WireCode> = WireCode::from_name("things::gone");
        let _: &'static ::core::primitive::str = WireCode::ThingsGone.as_str();
        let _: WireCode = WireCode::UNDECLARED;
        let _: ::core::result::Result<treat::ApiResponse<(), ()>, T> = ::core::convert::Into::into(T::Gone);
        let _: ::core::result::Result<treat::ApiResponse<(), ()>, M> = ::core::convert::Into::into(M::Gone);
    }
}

/// `root_span!` from a custom `RootSpanBuilder`, path-qualified and with no
/// `use` of the macro: both arms must reach everything through `$crate`, and
/// the expansion must not depend on the caller's features (see the
/// `telemetry` feature in Cargo.toml) or on the names it shadows here.
#[allow(non_camel_case_types)]
mod root_span_hygiene {
    use actix_web::dev::{ServiceRequest, ServiceResponse};

    struct Cow;
    struct Into;
    struct RequestId;
    mod std {}
    mod private {}
    #[allow(unused_macros)]
    macro_rules! format {
        ($($tt:tt)*) => {
            compile_error!("generated code must not call a local `format!`")
        };
    }
    #[allow(unused_macros)]
    macro_rules! root_span {
        ($($tt:tt)*) => {
            compile_error!("generated code must not call a local `root_span!`")
        };
    }

    struct Bare;

    impl treat::RootSpanBuilder for Bare {
        fn on_request_start(request: &ServiceRequest) -> tracing::Span {
            treat::root_span!(request)
        }

        fn on_request_end<B>(
            span: tracing::Span,
            outcome: &::core::result::Result<ServiceResponse<B>, actix_web::Error>,
        ) {
            <treat::DefaultRootSpanBuilder as treat::RootSpanBuilder>::on_request_end(span, outcome)
        }
    }

    struct WithFields;

    impl treat::RootSpanBuilder for WithFields {
        fn on_request_start(request: &ServiceRequest) -> tracing::Span {
            treat::root_span!(request, tenant = tracing::field::Empty, region = "eu")
        }

        fn on_request_end<B>(
            span: tracing::Span,
            outcome: &::core::result::Result<ServiceResponse<B>, actix_web::Error>,
        ) {
            <treat::DefaultRootSpanBuilder as treat::RootSpanBuilder>::on_request_end(span, outcome)
        }
    }

    fn _assert_loggers() {
        let _ = treat::Logger::<Bare>::new();
        let _ = treat::Logger::<WithFields>::new().error_logger(treat::error_log_for::<&'static str>);
    }
}
