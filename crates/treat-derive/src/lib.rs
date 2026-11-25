use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;

mod api_error;
mod api_error_code;
mod from_error_message;
mod serde_name;
#[cfg(test)]
mod tests;
mod wire_enum;

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
/// Every variant needs a `#[code]`, exactly one is the `#[code("_")]`
/// catch-all, and a concrete code may be claimed by one variant only; the
/// build fails otherwise. A generic enum is supported: each field type must
/// then be `From<ErrorMessage>`, which the impls require.
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
/// Tag every variant with `#[code("...")]` (a variant without one fails the
/// build, naming the variant); the variant's `Display` becomes the error
/// message. Optionally mark at most one variant `#[catch_all]` — a tuple
/// variant with exactly one field, an `erris::Report` — and it also gets a
/// `From<erris::Report>` impl, so `?` on a report produces your enum. Generates
/// `From<YourEnum> for ApiError`.
///
/// A generic enum is supported; the conversion requires `YourEnum<..>:
/// Display`, so it exists wherever the `thiserror` impl does.
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
/// Generates `Display` (the wire code: `#[code("...")]`, else the name serde
/// writes — the variant's `#[serde(rename)]` or the enum's
/// `#[serde(rename_all)]` — else the variant name, a raw identifier
/// unprefixed) plus `From<YourEnum> for ApiError<YourEnum>`.
/// `#[message("...")]` is a format string interpolated over the variant's
/// fields; tuple fields are named `arg_0`, `arg_1`, ...
///
/// Prefer setting `#[code("...")]` on every variant: without it or a serde
/// rename the code is the variant name verbatim (`NotFound`, not `not_found`),
/// which does not match the
/// `snake_case` convention used elsewhere here, and it silently changes if the
/// Rust variant is renamed. The default is kept for backwards compatibility.
///
/// `#[status(404)]` maps a variant onto a transport HTTP status. The generated
/// `From` applies it, and an [`ApiErrorStatus`] impl is emitted as soon as any
/// variant carries one, so `with_code_status()` works too; variants without the
/// attribute report the crate default.
///
/// A message binds only the fields it reads, so a variant whose message skips
/// a field raises no `unused_variables` warning. A field that neither the
/// message nor any other code reads is then reported by `dead_code` when the
/// enum is not public (a derived `Debug` is not a read): read it, drop it, or
/// put `#[allow(dead_code)]` on that field. A generic enum is supported:
/// the conversion into `ApiError<YourEnum<..>>` requires `YourEnum<..>` to be a
/// `'static` `ApiErrorCode`, so put the bounds a message needs (`T: Display`)
/// on the enum.
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

/// Give an enum of unit variants its wire names: `treat::rpc::WireEnum`.
///
/// Generates `ALL` (every variant, in declaration order), `from_name` and
/// `as_str`. The trait requires `Copy`, so derive that too. A variant's wire
/// name is, in order:
///
/// 1. `#[code("...")]` — the attribute `ApiErrorCode` already reads for
///    `Display`, so one attribute serves both derives;
/// 2. the name serde writes: `#[serde(rename = "...")]`, else the enum's
///    `#[serde(rename_all = "...")]` applied to the identifier;
/// 3. the identifier, a raw one unprefixed (`r#type` is `type`), as serde
///    writes it.
///
/// When `#[code]` and a serde `rename` / `rename_all` both name the variant
/// they must agree — the build fails otherwise, since the value would read one
/// way in an error envelope and another in a JSON payload. Two variants with
/// one wire name fail too, and so does a variant serde would write and read
/// under different names (`serialize` / `deserialize` halves of `rename` or
/// `rename_all` that differ, where the variant does not spell its own name).
///
/// Only those keys are read. `#[serde(alias)]`, `#[serde(other)]` and
/// `#[serde(skip_*)]` are not mirrored: `from_name` accepts exactly the wire
/// names, and `ALL` lists every variant, skipped or not. `ApiErrorCode`'s
/// `Display` reads `#[code]`, serde's `rename` / `rename_all` and the
/// identifier in the same order, so `Display` and `as_str` always write one
/// string, and serde writes it too wherever serde is told a name. A `#[code]`
/// serde is not told about is the wire name, while serde still writes the
/// identifier: add a matching `#[serde(rename)]` if the enum also travels as a
/// payload.
///
/// Mark one variant `#[undeclared]` to also implement `treat::rpc::Undeclared`:
/// the value `Narrow::narrow` reports a code the set does not declare as.
///
/// ```ignore
/// use treat::prelude::*;
///
/// #[derive(Debug, Clone, Copy, PartialEq, ApiErrorCode, treat::rpc::WireEnum)]
/// enum GetCodes {
///     #[code("things::gone")]
///     ThingsGone,
///     #[code("internal_error")]
///     #[undeclared]
///     InternalError,
/// }
/// ```
#[proc_macro_derive(WireEnum, attributes(code, undeclared))]
pub fn derive_wire_enum(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input);
    wire_enum::derive(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

/// `generics` with `predicates` added to its `where` clause.
///
/// The derives use it to bound a generic enum's impls on what the generated
/// code needs, so an impl exists exactly for the instantiations that satisfy it.
fn with_predicates(
    generics: &syn::Generics,
    predicates: impl IntoIterator<Item = syn::WherePredicate>,
) -> syn::Generics {
    let mut generics = generics.clone();
    generics.make_where_clause().predicates.extend(predicates);
    generics
}

/// `generics` with `params` appended. The enum's own lifetimes stay first, as
/// Rust requires of a parameter list.
fn with_type_params(generics: &syn::Generics, params: impl IntoIterator<Item = syn::GenericParam>) -> syn::Generics {
    let mut generics = generics.clone();
    generics.params.extend(params);
    generics
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
