use crate::{ApiResponse, ResponseData};
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display, Formatter};

/// Where an error originated, mirroring JSON:API's `error.source`. Each field is
/// optional; set the one that fits: a JSON Pointer into the request body, a query
/// `parameter`, or a request `header` name.
///
/// ```
/// let src = leto_core::ErrorSource::default().with_pointer("/data/attributes/email");
/// assert_eq!(src.pointer.as_deref(), Some("/data/attributes/email"));
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorSource {
    /// JSON Pointer (RFC 6901) to the offending value, e.g. `/data/attributes/email`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    /// Name of the query parameter that caused the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter: Option<String>,
    /// Name of the request header that caused the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
}

impl ErrorSource {
    pub fn with_pointer(mut self, pointer: impl Into<String>) -> Self {
        self.pointer = Some(pointer.into());
        self
    }

    pub fn with_parameter(mut self, parameter: impl Into<String>) -> Self {
        self.parameter = Some(parameter.into());
        self
    }

    pub fn with_header(mut self, header: impl Into<String>) -> Self {
        self.header = Some(header.into());
        self
    }

    /// `true` when no locator field is set — used to skip serialization.
    pub fn is_empty(&self) -> bool {
        self.pointer.is_none() && self.parameter.is_none() && self.header.is_none()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorMessage {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// RFC 9457 `type`: a URI identifying the error kind (link to docs).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_uri: Option<String>,
    /// RFC 9457 `instance`: a URI/id for this specific occurrence (often a request id).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    /// Where the error originated (JSON:API `source`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ErrorSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

impl ErrorMessage {
    #[inline]
    pub fn into_response<T: ResponseData, M: ResponseData>(self) -> ApiResponse<T, M> {
        crate::failure(self)
    }

    pub fn to_error_message(&self) -> ErrorMessage {
        self.clone()
    }
}

impl From<ErrorMessage> for serde_json::Value {
    fn from(err: ErrorMessage) -> serde_json::Value {
        let mut map = serde_json::Map::with_capacity(6);
        map.insert("code".into(), err.code.into());

        if let Some(message) = err.message {
            map.insert("message".into(), message.into());
        }

        if let Some(type_uri) = err.type_uri {
            map.insert("type".into(), type_uri.into());
        }

        if let Some(instance) = err.instance {
            map.insert("instance".into(), instance.into());
        }

        if let Some(source) = err.source.filter(|s| !s.is_empty()) {
            map.insert(
                "source".into(),
                serde_json::to_value(source).unwrap_or(serde_json::Value::Null),
            );
        }

        if let Some(meta) = err.meta {
            map.insert("meta".into(), meta);
        }

        map.into()
    }
}

impl IntoIterator for ErrorMessage {
    type Item = ErrorMessage;
    type IntoIter = std::iter::Once<ErrorMessage>;

    fn into_iter(self) -> Self::IntoIter {
        std::iter::once(self)
    }
}

impl Display for ErrorMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "leto error: {}", self.code)?;
        if let Some(message) = &self.message
            && !message.is_empty()
        {
            write!(f, ", message: {message}")?;
        }
        Ok(())
    }
}
