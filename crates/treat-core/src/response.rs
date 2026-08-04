use crate::{ApiError, ApiErrorCode, ErrorMessage};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[cfg(not(feature = "openapi"))]
pub trait ResponseData: Serialize {}
#[cfg(not(feature = "openapi"))]
impl<T: Serialize> ResponseData for T {}

#[cfg(feature = "openapi")]
pub trait ResponseData: Serialize + utoipa::ToSchema {}
#[cfg(feature = "openapi")]
impl<T: Serialize + utoipa::ToSchema> ResponseData for T {}

pub type NoData = ();
pub type NoMeta = ();

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ApiResponse<T: ResponseData = NoData, M: ResponseData = NoMeta> {
    pub data: Option<T>,

    pub meta: Option<M>,

    // `default` lets a document without `errors` deserialize to an empty vec; the
    // hand-written `Serialize` below owns when the field is emitted.
    #[serde(default)]
    pub errors: Vec<ErrorMessage>,
}

// `Serialize` is hand-written (not derived) to enforce the JSON:API envelope
// invariants, which a per-field `skip_serializing_if` cannot express because the
// presence of `data` depends on whether `errors` is empty:
//
//   * `errors` is emitted only when non-empty — never `"errors": []`.
//   * a success document (no errors) always carries `data`, even as `"data": null`;
//   * an error document (non-empty errors) omits `data` entirely.
//
// So `data` and `errors` never coexist on the wire, whatever the in-memory struct
// holds (e.g. after a fluent builder set both).
impl<T: ResponseData, M: ResponseData> Serialize for ApiResponse<T, M> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let is_error = !self.errors.is_empty();
        let len = 1 + self.meta.is_some() as usize; // `data` xor `errors`, plus optional `meta`
        let mut state = serializer.serialize_struct("ApiResponse", len)?;
        if !is_error {
            // Success: `data` is always present, serializing `None` as `null`.
            state.serialize_field("data", &self.data)?;
        }
        if let Some(meta) = &self.meta {
            state.serialize_field("meta", meta)?;
        }
        if is_error {
            state.serialize_field("errors", &self.errors)?;
        }
        state.end()
    }
}

/// The error-only shape of the envelope (`{ "errors": [...] }`), handy as the
/// `body` in `#[utoipa::path(responses((status = 400, body = ErrorResponse)))]`
/// where the generic [`ApiResponse`] would be awkward to name. Serializes
/// identically to an [`ApiResponse`] that carries only errors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorResponse {
    pub errors: Vec<ErrorMessage>,
}

impl ErrorResponse {
    pub fn new(errors: impl IntoIterator<Item = ErrorMessage>) -> Self {
        Self {
            errors: errors.into_iter().collect(),
        }
    }
}

impl<T: ResponseData, M: ResponseData> ApiResponse<T, M> {
    /// The [`X_RPC_STATUS`](crate::rpc_status::X_RPC_STATUS) value for this
    /// envelope: [`ERROR`](crate::rpc_status::ERROR) when it carries errors,
    /// [`OK`](crate::rpc_status::OK) otherwise.
    ///
    /// Keyed on the same `errors.is_empty()` test that `Serialize` uses to pick
    /// between a `data` and an `errors` document, so the header can never
    /// disagree with the body.
    #[cfg(feature = "rpc-status-header")]
    pub fn rpc_status(&self) -> &'static str {
        if self.errors.is_empty() {
            crate::rpc_status::OK
        } else {
            crate::rpc_status::ERROR
        }
    }

    pub fn has_error_code(&self, code: impl ApiErrorCode) -> Option<&ErrorMessage> {
        let code = code.to_string();
        self.errors.iter().find(|v| v.code == code)
    }

    pub fn first_error_code(&self) -> Option<&str> {
        self.errors.first().map(|v| v.code.as_str())
    }

    pub fn last_error_code(&self) -> Option<&str> {
        self.errors.last().map(|v| v.code.as_str())
    }

    pub fn with_meta(mut self, meta: M) -> Self {
        self.meta = Some(meta);
        self
    }

    pub fn with_data(mut self, data: T) -> Self {
        self.data = Some(data);
        self
    }

    pub fn with_errors(mut self, errs: impl IntoIterator<Item = ErrorMessage>) -> Self {
        self.errors = errs.into_iter().collect();
        self
    }
}

pub fn success<T: ResponseData>(data: T) -> ApiResponse<T, NoMeta> {
    ApiResponse {
        data: Some(data),
        meta: None,
        errors: [].into(),
    }
}

/// Like [`success`], but with typed `meta` — `success` fixes `M` to [`NoMeta`],
/// so this is the way to attach a custom meta type (e.g. `meta_slots::Pagination`).
pub fn success_with_meta<T: ResponseData, M: ResponseData>(data: T, meta: M) -> ApiResponse<T, M> {
    ApiResponse {
        data: Some(data),
        meta: Some(meta),
        errors: [].into(),
    }
}

pub fn failure<T: ResponseData, Meta: ResponseData>(
    errors: impl IntoIterator<Item = ErrorMessage>,
) -> ApiResponse<T, Meta> {
    ApiResponse {
        data: None,
        meta: None,
        errors: errors.into_iter().collect(),
    }
}

impl<C: ApiErrorCode> From<ApiError<C>> for ApiResponse {
    #[track_caller]
    fn from(err: ApiError<C>) -> Self {
        err.into_api_response()
    }
}

impl<C: ApiErrorCode> From<&ApiError<C>> for ApiResponse {
    #[track_caller]
    fn from(err: &ApiError<C>) -> Self {
        err.into_api_response()
    }
}

impl<T: ResponseData, C: ApiErrorCode> From<std::result::Result<T, ApiError<C>>> for ApiResponse<T, NoMeta> {
    #[track_caller]
    fn from(res: std::result::Result<T, ApiError<C>>) -> Self {
        match res {
            Ok(data) => success(data),
            Err(err) => err.into_api_response(),
        }
    }
}
