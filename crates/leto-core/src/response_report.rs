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

    pub fn inner<'a, E: From<&'a ErrorMessage> + Into<erris::Report>>(&'a self) -> erris::Result<Option<&'a T>, E> {
        if let Some(err) = self.errors.first() {
            return Err(E::from(err));
        }
        Ok(self.data.as_ref())
    }

    pub fn into_inner<E: From<ErrorMessage> + Into<erris::Report>>(self) -> erris::Result<Option<T>, E> {
        if let Some(err) = self.errors.into_iter().next() {
            return Err(E::from(err));
        }
        Ok(self.data)
    }

    pub fn inner_data(&self) -> erris::Result<&T> {
        if let Some(err) = self.errors.first() {
            return Err(erris::report!("{err}"));
        }
        self.data
            .as_ref()
            .ok_or_else(|| erris::report!("missing field data in response"))
    }

    pub fn into_inner_data(self) -> erris::Result<T> {
        if let Some(err) = self.errors.first() {
            return Err(erris::report!("{err}"));
        }
        self.data
            .ok_or_else(|| erris::report!("missing field data in response"))
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
