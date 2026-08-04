use crate::{ApiError, ApiErrorCode, error};

pub trait OkOrError<T, C: ApiErrorCode> {
    #[track_caller]
    fn ok_or_api_error(self, code: C) -> Result<T, ApiError<C>>;

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
