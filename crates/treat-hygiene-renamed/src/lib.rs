//! Compile-only regression: the derive macros must resolve the runtime crate by
//! the name the *dependant* uses. Here the facade is `envelope`, never `treat`, so
//! any hardcoded `treat::` path in generated code breaks this crate's build.
#![allow(dead_code)]

#[derive(Clone, Debug, envelope::ApiErrorCode)]
enum OrderCode {
    #[message("order {id} not found")]
    NotFound { id: u64 },
    #[code("order.already_paid")]
    AlreadyPaid,
}

#[derive(Debug, thiserror::Error, envelope::ApiError)]
enum ServiceError {
    #[error("forbidden")]
    #[code("forbidden")]
    Forbidden,

    #[catch_all]
    #[error("internal")]
    #[code("internal")]
    Internal(#[source] envelope::erris::Report),
}

#[derive(Debug, envelope::FromErrorMessage)]
enum ClientError {
    #[code("not_found")]
    NotFound(envelope::ErrorMessage),
    #[code("_")]
    Other(envelope::ErrorMessage),
}

fn _assert_impls() {
    let _: envelope::ApiError<OrderCode> = OrderCode::NotFound { id: 1 }.into();
    let _: envelope::ApiError<OrderCode> = OrderCode::AlreadyPaid.into();
    let _: Result<envelope::ApiResponse<(), ()>, OrderCode> = OrderCode::AlreadyPaid.into();
    let _: envelope::ApiError = ServiceError::Forbidden.into();
    let _: ServiceError = envelope::erris::report!("boom").into();
    let _: ClientError = envelope::error("not_found").into();
}

#[derive(Clone, Copy, Debug, envelope::rpc::WireEnum)]
enum WireCode {
    #[code("things::gone")]
    ThingsGone,
    #[undeclared]
    InternalError,
}

fn _assert_wire_impls() {
    use envelope::rpc::{Undeclared, WireEnum};
    let _: Option<WireCode> = WireCode::from_name("things::gone");
    let _: WireCode = WireCode::UNDECLARED;
}

/// Every derive expanded next to local items that shadow the std prelude, the
/// primitive types the generated signatures name, the `format!` / `write!`
/// macros and the `std` crate itself. The generated code must reach each of
/// them through an absolute `::core` / `::std` path, so none of these local
/// names can capture it.
#[allow(non_camel_case_types)]
mod shadowed_names {
    use envelope::{ApiErrorCode, ErrorMessage, FromErrorMessage};

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

    #[derive(Debug, thiserror::Error, envelope::ApiError)]
    enum ServiceError {
        #[error("forbidden")]
        #[code("forbidden")]
        #[status(403)]
        Forbidden,

        #[catch_all]
        #[error("internal")]
        #[code("internal")]
        Internal(#[source] envelope::erris::Report),
    }

    #[derive(Debug, FromErrorMessage)]
    enum ClientError {
        #[code("not_found")]
        NotFound(ErrorMessage),
        #[code("_")]
        Other { message: ErrorMessage },
    }

    #[derive(Clone, Copy, Debug, envelope::rpc::WireEnum)]
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
        use envelope::rpc::{Undeclared, WireEnum};
        let _: envelope::ApiError<OrderCode> = ::core::convert::Into::into(OrderCode::NotFound { id: 1, value: 2 });
        let _: envelope::ApiError<OrderCode> = ::core::convert::Into::into(OrderCode::Bad(3));
        let _: ::core::result::Result<envelope::ApiResponse<(), ()>, OrderCode> =
            ::core::convert::Into::into(OrderCode::AlreadyPaid);
        let _: ::core::primitive::u16 = envelope::ApiErrorStatus::status_code(&OrderCode::AlreadyPaid);
        let _: envelope::ApiError = ::core::convert::Into::into(ServiceError::Forbidden);
        let _: ServiceError = ::core::convert::Into::into(envelope::erris::report!("boom"));
        let _: ClientError = ::core::convert::Into::into(envelope::error("not_found"));
        let _: ::core::option::Option<WireCode> = WireCode::from_name("things::gone");
        let _: &'static ::core::primitive::str = WireCode::ThingsGone.as_str();
        let _: WireCode = WireCode::UNDECLARED;
        let _: ::core::result::Result<envelope::ApiResponse<(), ()>, T> = ::core::convert::Into::into(T::Gone);
        let _: ::core::result::Result<envelope::ApiResponse<(), ()>, M> = ::core::convert::Into::into(M::Gone);
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

    impl envelope::RootSpanBuilder for Bare {
        fn on_request_start(request: &ServiceRequest) -> tracing::Span {
            envelope::root_span!(request)
        }

        fn on_request_end<B>(
            span: tracing::Span,
            outcome: &::core::result::Result<ServiceResponse<B>, actix_web::Error>,
        ) {
            <envelope::DefaultRootSpanBuilder as envelope::RootSpanBuilder>::on_request_end(span, outcome)
        }
    }

    struct WithFields;

    impl envelope::RootSpanBuilder for WithFields {
        fn on_request_start(request: &ServiceRequest) -> tracing::Span {
            envelope::root_span!(request, tenant = tracing::field::Empty, region = "eu")
        }

        fn on_request_end<B>(
            span: tracing::Span,
            outcome: &::core::result::Result<ServiceResponse<B>, actix_web::Error>,
        ) {
            <envelope::DefaultRootSpanBuilder as envelope::RootSpanBuilder>::on_request_end(span, outcome)
        }
    }

    fn _assert_loggers() {
        let _ = envelope::Logger::<Bare>::new();
        let _ = envelope::Logger::<WithFields>::new().error_logger(envelope::error_log_for::<&'static str>);
    }
}
