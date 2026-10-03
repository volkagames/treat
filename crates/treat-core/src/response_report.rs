use crate::{ApiError, ApiResponse, ErrorMessage, ResponseData};

impl<T: ResponseData, M: ResponseData> ApiResponse<T, M> {
    #[inline]
    pub fn as_result(self) -> Result<ApiResponse<T, M>, ApiError> {
        Ok(self)
    }

    #[inline]
    pub fn ok(&self) -> Option<&T> {
        self.data.as_ref()
    }

    #[inline]
    pub fn err(&self) -> Option<&ErrorMessage> {
        self.errors.first()
    }

    // `erris::Result` is spelled out as std's here and below: with erris `tracked` on it becomes
    // `TrackedResult`, which takes no error type, so a generic `E` needs the std result.
    pub fn inner<'a, E: From<&'a ErrorMessage> + Into<erris::Report>>(
        &'a self,
    ) -> std::result::Result<Option<&'a T>, E> {
        if let Some(err) = self.errors.first() {
            return Err(E::from(err));
        }
        Ok(self.data.as_ref())
    }

    pub fn into_inner<E: From<ErrorMessage> + Into<erris::Report>>(self) -> std::result::Result<Option<T>, E> {
        if let Some(err) = self.errors.into_iter().next() {
            return Err(E::from(err));
        }
        Ok(self.data)
    }

    // Variants are named through `erris::Result` so they resolve to the tracking ones when erris
    // `tracked` is on, and to std's otherwise.
    pub fn inner_data(&self) -> erris::Result<&T> {
        if let Some(err) = self.errors.first() {
            return erris::Result::Err(erris::report!("{err}"));
        }
        match self.data.as_ref() {
            Some(data) => erris::Result::Ok(data),
            None => erris::Result::Err(erris::report!("missing field data in response")),
        }
    }

    pub fn into_inner_data(self) -> erris::Result<T> {
        if let Some(err) = self.errors.first() {
            return erris::Result::Err(erris::report!("{err}"));
        }
        match self.data {
            Some(data) => erris::Result::Ok(data),
            None => erris::Result::Err(erris::report!("missing field data in response")),
        }
    }
}

impl<T: ResponseData, M: ResponseData> From<T> for ApiResponse<T, M> {
    fn from(data: T) -> Self {
        ApiResponse {
            data: Some(data),
            meta: None,
            errors: [].into(),
        }
    }
}
