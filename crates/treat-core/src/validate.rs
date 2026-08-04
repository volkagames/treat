//! Turn `validator::ValidationErrors` into field-level [`ErrorMessage`]s that
//! carry a JSON Pointer to each offending field.
//!
//! Enabled by the `validator` feature. Pairs with the field-level locators
//! (Phase 9 F1): each field's pointer lands in `ErrorMessage.source.pointer`.

use crate::{ApiResponse, ErrorMessage, NoData, failure};
use validator::{Validate, ValidationErrors, ValidationErrorsKind};

/// Flatten a [`ValidationErrors`] tree into one [`ErrorMessage`] per field
/// violation. Nested structs and lists are walked recursively; the JSON Pointer
/// reflects the nesting (`/address/zip`, `/items/0/name`).
///
/// The `code` is the validator rule code (`"length"`, `"email"`, ...), the
/// `message` is the rule message when present.
pub fn validation_error_messages(errors: &ValidationErrors) -> Vec<ErrorMessage> {
    let mut output = Vec::new();
    collect(errors, &mut String::new(), &mut output);
    output
}

fn collect(errors: &ValidationErrors, prefix: &mut String, output: &mut Vec<ErrorMessage>) {
    for (field, kind) in errors.errors() {
        let base = prefix.len();
        prefix.push('/');
        prefix.push_str(&escape_token(field));

        match kind {
            ValidationErrorsKind::Field(field_errors) => {
                for err in field_errors {
                    output.push(ErrorMessage {
                        code: err.code.to_string(),
                        message: err.message.as_ref().map(ToString::to_string),
                        type_uri: None,
                        instance: None,
                        source: Some(crate::ErrorSource::default().with_pointer(prefix.clone())),
                        meta: None,
                    });
                }
            }
            ValidationErrorsKind::Struct(nested) => collect(nested, prefix, output),
            ValidationErrorsKind::List(items) => {
                for (index, nested) in items {
                    let inner = prefix.len();
                    prefix.push('/');
                    prefix.push_str(&index.to_string());
                    collect(nested, prefix, output);
                    prefix.truncate(inner);
                }
            }
        }

        prefix.truncate(base);
    }
}

/// Escape a JSON Pointer reference token per RFC 6901: `~` → `~0`, `/` → `~1`.
fn escape_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Validate `self` and, on failure, build a failure envelope with one entry in
/// `errors[]` per field violation (each with a JSON Pointer locator).
///
/// Returns `Ok(())` when valid. The `Err` is an [`ApiResponse`], which every
/// framework adapter renders; a handler whose error type is `ApiResponse` can
/// bubble it with `?` (and `?` on a [`crate::ApiError`] unifies via the existing
/// `From<ApiError> for ApiResponse`).
///
/// ```
/// # use validator::Validate;
/// # use treat_core::ValidateApi;
/// #[derive(Debug, Validate)]
/// struct Body { #[validate(email)] email: String }
/// let err = Body { email: "nope".into() }.validate_api().unwrap_err();
/// assert_eq!(err.errors[0].code, "email");
/// assert_eq!(
///     err.errors[0].source.as_ref().and_then(|s| s.pointer.as_deref()),
///     Some("/email"),
/// );
/// ```
pub trait ValidateApi: Validate {
    fn validate_api(&self) -> Result<(), ApiResponse<NoData>> {
        match self.validate() {
            Ok(()) => Ok(()),
            Err(errors) => Err(failure(validation_error_messages(&errors))),
        }
    }
}

impl<T: Validate> ValidateApi for T {}

/// A value proven valid: it can only be constructed by running validation
/// ([`Validated::new`] or the `ApiValidated` extractor), so holding one is a
/// type-level guarantee that [`Validate::validate`] passed. There is no way to
/// re-validate a `Validated<T>`, which rules out redundant validation.
///
/// ```
/// # use validator::Validate;
/// # use treat_core::Validated;
/// #[derive(Validate)]
/// struct Body { #[validate(email)] email: String }
/// let ok = Validated::new(Body { email: "a@b.com".into() });
/// assert!(ok.is_ok());
/// let bad = Validated::new(Body { email: "nope".into() });
/// assert!(bad.is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Validated<T>(T);

impl<T: Validate> Validated<T> {
    /// Validate `value` and, on success, wrap it as proof of validation. On
    /// failure returns the same failure envelope as [`ValidateApi::validate_api`].
    pub fn new(value: T) -> Result<Self, ApiResponse<NoData>> {
        value.validate_api()?;
        Ok(Self(value))
    }
}

impl<T> Validated<T> {
    /// Borrow the validated value.
    pub fn get(&self) -> &T {
        &self.0
    }

    /// Unwrap the validated value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> std::ops::Deref for Validated<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}
