use crate::{ApiError, ApiErrorCode, error};

pub trait WrapApiError<T, C: ApiErrorCode> {
    /// Wrap the error with `code`, keeping it as the source.
    ///
    /// Builds the error directly from the code, so a `#[message(...)]` declared on
    /// a `#[derive(ApiErrorCode)]` variant is **not** applied — the resulting entry
    /// has no message. Use [`wrap_api_code`](Self::wrap_api_code) to pick the
    /// declared message up, or
    /// [`wrap_api_error_and_message`](Self::wrap_api_error_and_message) to supply
    /// one here.
    #[track_caller]
    fn wrap_api_error(self, code: C) -> Result<T, ApiError<C>>;

    /// Wrap the error with `code`, applying the code's declared message.
    ///
    /// Goes through `From<C> for ApiError<C>` — the impl `#[derive(ApiErrorCode)]`
    /// generates — so `#[message(...)]` is applied and the message stays declared
    /// next to the code instead of being restated at each call site. The original
    /// error is kept as the source either way.
    ///
    /// ```
    /// use treat_core::prelude::*;
    /// # use treat_core::{ApiError, error};
    /// # #[derive(Clone, Debug)]
    /// # enum Code { Internal }
    /// # impl std::fmt::Display for Code {
    /// #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "internal") }
    /// # }
    /// # impl From<Code> for ApiError<Code> {
    /// #     fn from(c: Code) -> Self { error(c).with_message("the request could not be completed") }
    /// # }
    /// let failed: Result<(), std::io::Error> = Err(std::io::Error::other("disk"));
    /// let err = failed.wrap_api_code(Code::Internal).expect_err("wrapped");
    ///
    /// assert_eq!(err.message().map(|m| m.as_ref()), Some("the request could not be completed"));
    /// assert!(err.source().is_some(), "the cause is kept for the logs");
    /// ```
    #[track_caller]
    fn wrap_api_code(self, code: C) -> Result<T, ApiError<C>>
    where
        C: Into<ApiError<C>>;

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
    fn wrap_api_code(self, code: C) -> Result<T, ApiError<C>>
    where
        C: Into<ApiError<C>>,
    {
        // TODO FIXME https://github.com/rust-lang/rust/issues/87417
        match self {
            Ok(v) => Ok(v),
            Err(err) => Err(code.into().with_source(err.into_report())),
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
    /// Attach `code` to this report.
    ///
    /// Builds the error directly from the code, so a `#[message(...)]` declared on
    /// a `#[derive(ApiErrorCode)]` variant is **not** applied. Use
    /// [`with_api_code`](Self::with_api_code) to pick the declared message up.
    #[track_caller]
    fn with_error_code<C: ApiErrorCode>(self, code: C) -> ApiError<C> {
        error(code).with_source(self.into())
    }

    /// Attach `code` to this report, applying the code's declared message.
    ///
    /// The `From<C> for ApiError<C>` counterpart of
    /// [`with_error_code`](Self::with_error_code); see
    /// [`WrapApiError::wrap_api_code`] for the rationale.
    #[track_caller]
    fn with_api_code<C: ApiErrorCode + Into<ApiError<C>>>(self, code: C) -> ApiError<C> {
        code.into().with_source(self.into())
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
