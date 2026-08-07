use crate::{ApiError, ApiErrorCode, ApiResponse, NoMeta, ResponseData, error, success};

pub trait ApiErrorTrack<T, C: ApiErrorCode> {
    #[track_caller]
    fn track_api_error(self) -> Result<T, ApiError<C>>;
}

impl<T, C: ApiErrorCode> ApiErrorTrack<T, C> for Result<T, ApiError<C>> {
    #[track_caller]
    fn track_api_error(self) -> Result<T, ApiError<C>> {
        match self {
            Ok(t) => Ok(t),
            Err(e) => Err(e.track()),
        }
    }
}

pub trait ApiResponseTrack<T: ResponseData, C: ApiErrorCode> {
    /// Turn any fallible call into a handler result in one step: wrap the value
    /// in [`success`], or convert the error into `C`'s default code.
    ///
    /// The counterpart of [`ApiErrorTrack::track_api_error`] for a call that has
    /// not been mapped to an [`ApiError`] yet. The original error is kept as the
    /// source, so nothing is lost from the logs; only the wire code is defaulted.
    ///
    /// ```
    /// use treat_core::prelude::*;
    /// # use treat_core::{ApiError, ApiResponse};
    /// # #[derive(Clone, Debug, Default)]
    /// # enum Code { #[default] Internal }
    /// # impl std::fmt::Display for Code {
    /// #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "internal") }
    /// # }
    /// fn handler() -> Result<ApiResponse<u8>, ApiError<Code>> {
    ///     let failed: Result<u8, std::io::Error> = Err(std::io::Error::other("disk"));
    ///     failed.track_api_response()
    /// }
    ///
    /// let err = handler().expect_err("defaulted");
    /// assert_eq!(err.code().to_string(), "internal");
    /// assert!(err.source().is_some(), "the cause is kept for the logs");
    /// ```
    #[track_caller]
    fn track_api_response(self) -> Result<ApiResponse<T, NoMeta>, ApiError<C>>
    where
        C: Default;
}

impl<T, E, C> ApiResponseTrack<T, C> for Result<T, E>
where
    T: ResponseData,
    E: erris::IntoReport + Send + Sync + 'static,
    C: ApiErrorCode,
{
    #[track_caller]
    fn track_api_response(self) -> Result<ApiResponse<T, NoMeta>, ApiError<C>>
    where
        C: Default,
    {
        // TODO FIXME https://github.com/rust-lang/rust/issues/87417
        match self {
            Ok(t) => Ok(success(t)),
            Err(e) => Err(error(C::default()).with_source(e.into_report())),
        }
    }
}
