use crate::{ApiError, ApiErrorCode, error};

pub trait OkOrError<T, C: ApiErrorCode> {
    /// Convert absence into an error carrying `code`.
    ///
    /// Builds the error directly from the code, so a `#[message(...)]` declared on
    /// a `#[derive(ApiErrorCode)]` variant is **not** applied — the resulting entry
    /// has no message. Use [`ok_or_api_code`](Self::ok_or_api_code) to pick the
    /// declared message up, or
    /// [`ok_or_api_error_with_message`](Self::ok_or_api_error_with_message) to
    /// supply one here.
    #[track_caller]
    fn ok_or_api_error(self, code: C) -> Result<T, ApiError<C>>;

    /// Convert absence into an error carrying `code` and the code's declared
    /// message.
    ///
    /// Goes through `From<C> for ApiError<C>` — the impl `#[derive(ApiErrorCode)]`
    /// generates — so `#[message(...)]` is applied and the message stays declared
    /// next to the code instead of being restated at each call site.
    ///
    /// ```
    /// use treat_core::prelude::*;
    /// # use treat_core::{ApiError, error};
    /// # #[derive(Clone, Debug)]
    /// # enum Code { NotFound }
    /// # impl std::fmt::Display for Code {
    /// #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "not_found") }
    /// # }
    /// # impl From<Code> for ApiError<Code> {
    /// #     fn from(c: Code) -> Self { error(c).with_message("user was not found") }
    /// # }
    /// let missing: Option<u32> = None;
    /// let err = missing.ok_or_api_code(Code::NotFound).expect_err("absent");
    ///
    /// assert_eq!(err.message().map(|m| m.as_ref()), Some("user was not found"));
    /// ```
    #[track_caller]
    fn ok_or_api_code(self, code: C) -> Result<T, ApiError<C>>
    where
        C: Into<ApiError<C>>;

    #[track_caller]
    fn ok_or_api_error_with_message(
        self,
        code: C,
        message: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<T, ApiError<C>>;
}

impl<T, C> OkOrError<T, C> for Option<T>
where
    C: ApiErrorCode,
{
    #[track_caller]
    fn ok_or_api_error(self, code: C) -> Result<T, ApiError<C>> {
        match self {
            Some(v) => Ok(v),
            None => Err(error(code)),
        }
    }

    #[track_caller]
    fn ok_or_api_code(self, code: C) -> Result<T, ApiError<C>>
    where
        C: Into<ApiError<C>>,
    {
        match self {
            Some(v) => Ok(v),
            None => Err(code.into()),
        }
    }

    #[track_caller]
    fn ok_or_api_error_with_message(
        self,
        code: C,
        message: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<T, ApiError<C>> {
        match self {
            Some(v) => Ok(v),
            None => Err(error(code).with_message(message.into())),
        }
    }
}

/// A `bool` guard carries no value, so the success type is `()` — `Ok(true)`
/// would always be `true` and tell the caller nothing. Use it as a bare guard:
/// `is_owner.ok_or_api_error("forbidden")?;`
impl<C> OkOrError<(), C> for bool
where
    C: ApiErrorCode,
{
    #[track_caller]
    fn ok_or_api_error(self, code: C) -> Result<(), ApiError<C>> {
        if self {
            return Ok(());
        };
        Err(error(code))
    }

    #[track_caller]
    fn ok_or_api_code(self, code: C) -> Result<(), ApiError<C>>
    where
        C: Into<ApiError<C>>,
    {
        if self {
            return Ok(());
        }
        Err(code.into())
    }

    #[track_caller]
    fn ok_or_api_error_with_message(
        self,
        code: C,
        message: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Result<(), ApiError<C>> {
        if self {
            return Ok(());
        }
        Err(error(code).with_message(message.into()))
    }
}
