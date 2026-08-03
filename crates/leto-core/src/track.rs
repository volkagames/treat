use crate::{ApiError, ApiErrorCode};

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
