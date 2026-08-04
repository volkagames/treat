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
