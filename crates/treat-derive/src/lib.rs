use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;

mod api_error;
mod api_error_code;
mod from_error_message;

use from_error_message::impl_from_error_message;

/// Resolve the path to the runtime crate the generated code must reference.
///
/// The macros cannot hardcode `treat::`: a dependant may reach the items through
/// `treat-core` directly (no facade in the tree), rename the dependency, or shadow
/// the name with a local module. `proc-macro-crate` reads the caller's manifest
/// and returns the real name, falling back to `treat-core` when only the core
/// crate is present.
fn runtime_path() -> proc_macro2::TokenStream {
    use proc_macro_crate::{FoundCrate, crate_name};

    // The facade re-exports every item the macros need, so prefer it; a
    // core-only dependant gets `treat_core`.
    for candidate in ["treat", "treat-core"] {
        match crate_name(candidate) {
            Ok(FoundCrate::Itself) => {
                // Expanding inside the runtime crate's own test/doc code.
                let ident = syn::Ident::new(candidate.replace('-', "_").as_str(), Span::call_site());
                return quote! { ::#ident };
            }
            Ok(FoundCrate::Name(name)) => {
                let ident = syn::Ident::new(&name.replace('-', "_"), Span::call_site());
                return quote! { ::#ident };
            }
            Err(_) => continue,
        }
    }

    // Neither manifest entry found (e.g. an unusual build layout): keep the
    // historical path so the error names a crate the user recognises.
    quote! { ::treat }
}

/// Map a wire [`ErrorMessage`] (or `ApiError`) back onto your own enum by `code`.
///
/// Each variant is a tuple holding an `ErrorMessage` and tagged with
/// `#[code("...")]`; the `#[code("_")]` variant is the catch-all for unknown
/// codes. Generates `From<ErrorMessage>`, `From<&ErrorMessage>`,
/// `From<ApiError>` and `From<&ApiError>`. Ideal on the client side after
/// deserializing a response.
///
/// ```ignore
/// use treat::prelude::*;
///
/// #[derive(Debug, FromErrorMessage)]
/// enum ClientError {
///     #[code("user_not_found")]
///     NotFound(ErrorMessage),
///     #[code("_")]
///     Other(ErrorMessage),
/// }
/// ```
///
/// [`ErrorMessage`]: ../treat/struct.ErrorMessage.html
#[proc_macro_derive(FromErrorMessage, attributes(code))]
pub fn from_error_message(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    impl_from_error_message(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

/// Map an existing (e.g. `thiserror`) enum onto `ApiError<&'static str>`.
///
/// Tag each variant with `#[code("...")]`; the variant's `Display` becomes the
/// error message. Mark exactly one tuple variant `#[catch_all]` — it must hold
/// an `erris::Report` and also gets a `From<erris::Report>` impl, so `?` on a
/// report produces your enum. Generates `From<YourEnum> for ApiError`.
///
/// Add `#[status(404)]` to carry a transport HTTP status alongside the code; the
/// generated conversion applies it, so the adapters return it without the call
/// site restating the status. Variants without it keep the crate default.
///
/// ```ignore
/// use treat::prelude::*;
/// use thiserror::Error;
///
/// #[derive(Debug, Error, ApiError)]
/// enum ServiceError {
///     #[error("access denied")]
///     #[code("forbidden")]
///     #[status(403)]
///     Forbidden,
///
///     #[catch_all]
///     #[error("internal error")]
///     #[code("internal")]
///     #[status(500)]
///     Internal(#[source] treat::erris::Report),
/// }
/// ```
#[proc_macro_derive(ApiError, attributes(code, catch_all, status))]
pub fn derive_api_error(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input);
    api_error::derive(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

/// Turn an enum into a **typed error code** usable as `ApiError<YourEnum>`.
///
/// Generates `Display` (the wire code — the variant name by default, overridable
/// with `#[code("...")]`) plus `From<YourEnum> for ApiError<YourEnum>`.
/// `#[message("...")]` is a format string interpolated over the variant's
/// fields; tuple fields are named `arg_0`, `arg_1`, ...
///
/// Prefer setting `#[code("...")]` on every variant: the default is the variant
/// name verbatim (`NotFound`, not `not_found`), which does not match the
/// `snake_case` convention used elsewhere here, and it silently changes if the
/// Rust variant is renamed. The default is kept for backwards compatibility.
///
/// `#[status(404)]` maps a variant onto a transport HTTP status. The generated
/// `From` applies it, and an [`ApiErrorStatus`] impl is emitted as soon as any
/// variant carries one, so `with_code_status()` works too; variants without the
/// attribute report the crate default.
///
/// ```ignore
/// use treat::prelude::*;
///
/// #[derive(Clone, Debug, PartialEq, ApiErrorCode)]
/// enum OrderError {
///     #[message("order {id} not found")]
///     #[status(404)]
///     NotFound { id: u64 },
///     #[code("order.already_paid")]
///     AlreadyPaid,
/// }
///
/// let err: ApiError<OrderError> = OrderError::NotFound { id: 7 }.into();
/// assert_eq!(err.status(), 404);
/// ```
///
/// [`ApiErrorStatus`]: ../treat/trait.ApiErrorStatus.html
#[proc_macro_derive(ApiErrorCode, attributes(code, message, status))]
pub fn derive_api_error_code(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input);
    api_error_code::derive(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

/// Read a string-literal attribute (`#[code("...")]` / `#[message("...")]`).
///
/// Returns `Ok(None)` when absent. A malformed value yields a `syn::Error` (a
/// pointed diagnostic on the offending tokens) rather than a macro panic.
fn fetch_str_attr(attrs: &[syn::Attribute], name: &str) -> syn::Result<Option<String>> {
    let Some(attr) = attrs.iter().find(|attr| attr.path().is_ident(name)) else {
        return Ok(None);
    };
    attr.parse_args::<syn::LitStr>()
        .map_err(|err| {
            syn::Error::new(
                err.span(),
                format!("attribute `{name}` expects a string literal, e.g. #[{name}(\"...\")]"),
            )
        })
        .map(|lit| Some(lit.value()))
}

fn fetch_code_from_attrs(attrs: &[syn::Attribute]) -> syn::Result<Option<String>> {
    fetch_str_attr(attrs, "code")
}

fn fetch_message_from_attrs(attrs: &[syn::Attribute]) -> syn::Result<Option<String>> {
    fetch_str_attr(attrs, "message")
}

/// Mirrors `treat_core::VALID_STATUS_RANGE`; duplicated because a proc-macro
/// crate cannot depend on the runtime crate it generates code for.
const VALID_STATUS_RANGE: std::ops::RangeInclusive<u16> = 100..=599;

/// Read `#[status(404)]`, rejecting out-of-range values at compile time.
///
/// Returns `Ok(None)` when absent. Checking here rather than leaning on the
/// runtime `debug_assert` in `with_status` turns a typo into a build error.
fn fetch_status_from_attrs(attrs: &[syn::Attribute]) -> syn::Result<Option<u16>> {
    let Some(attr) = attrs.iter().find(|attr| attr.path().is_ident("status")) else {
        return Ok(None);
    };
    let lit = attr.parse_args::<syn::LitInt>().map_err(|err| {
        syn::Error::new(
            err.span(),
            "attribute `status` expects an integer literal, e.g. #[status(404)]",
        )
    })?;
    let status = lit.base10_parse::<u16>()?;
    if !VALID_STATUS_RANGE.contains(&status) {
        return Err(syn::Error::new_spanned(
            &lit,
            format!(
                "`status` must be a valid HTTP status code ({}..={})",
                VALID_STATUS_RANGE.start(),
                VALID_STATUS_RANGE.end(),
            ),
        ));
    }
    Ok(Some(status))
}
