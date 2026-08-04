use crate::{ApiError, ApiErrorCode, error};

pub trait WrapApiError<T, C: ApiErrorCode> {
    #[track_caller]
    fn wrap_api_error(self, code: C) -> Result<T, ApiError<C>>;

    #[track_caller]
    fn wrap_api_error_and_message(
        self,
        code: C,
        message: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<T, ApiError<C>>;

    #[track_caller]
    fn wrap_api_error_with<F, M>(self, f: F) -> Result<T, ApiError<C>>
    where
        F: FnOnce() -> (C, M),
        M: Into<std::borrow::Cow<'static, str>>;
}

impl<T, E, C> WrapApiError<T, C> for Result<T, E>
where
    E: erris::IntoReport + Send + Sync + 'static,
    C: ApiErrorCode,
{
    #[track_caller]
    fn wrap_api_error(self, code: C) -> Result<T, ApiError<C>> {
        // TODO FIXME https://github.com/rust-lang/rust/issues/87417
        // self.map_err(|err| error(code).with_source(err.into_report()))
        match self {
            Ok(v) => Ok(v),
            Err(err) => Err(error(code).with_source(err.into_report())),
        }
    }

    #[track_caller]
    fn wrap_api_error_and_message(
        self,
        code: C,
        message: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<T, ApiError<C>> {
        // TODO FIXME https://github.com/rust-lang/rust/issues/87417
        match self {
            Ok(v) => Ok(v),
            Err(err) => Err(error(code).with_message(message.into()).with_source(err.into_report())),
        }
    }

    #[track_caller]
    fn wrap_api_error_with<F, M>(self, f: F) -> Result<T, ApiError<C>>
    where
        F: FnOnce() -> (C, M),
        M: Into<std::borrow::Cow<'static, str>>,
    {
        // TODO FIXME https://github.com/rust-lang/rust/issues/87417
        match self {
            Ok(v) => Ok(v),
            Err(err) => {
                let (code, message) = f();
                Err(error(code).with_message(message.into()).with_source(err.into_report()))
            }
        }
    }
}

pub trait WithErrorCode: Into<erris::Report> {
    #[track_caller]
    fn with_error_code<C: ApiErrorCode>(self, code: C) -> ApiError<C> {
        error(code).with_source(self.into())
    }

    #[track_caller]
    fn with_error_code_and_message<C: ApiErrorCode>(
        self,
        code: C,
        message: impl Into<std::borrow::Cow<'static, str>>,
    ) -> ApiError<C> {
        error(code).with_message(message.into()).with_source(self.into())
    }
}

impl WithErrorCode for erris::Report {}
