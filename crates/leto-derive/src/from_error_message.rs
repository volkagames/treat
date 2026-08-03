use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Data::Enum;
use syn::spanned::Spanned;
use syn::{DataEnum, DeriveInput, Fields, Variant};

pub fn impl_from_error_message(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let enum_name = &input.ident;

    let Enum(DataEnum { ref variants, .. }) = input.data else {
        return Err(syn::Error::new_spanned(
            input,
            "#[derive(FromErrorMessage)] supports only enums",
        ));
    };

    // Partition the arms: concrete `#[code("...")]` variants keep their source
    // order, the single `#[code("_")]` catch-all is emitted LAST. A `match` is
    // top-to-bottom, so a catch-all declared before a concrete variant would
    // shadow it into an unreachable arm — and rustc suppresses `unreachable_pattern`
    // on macro-generated spans, so the misroute would be entirely silent.
    let mut concrete_arms: Vec<TokenStream2> = Vec::new();
    let mut catch_all_arm: Option<TokenStream2> = None;
    let mut catch_all_span: Option<proc_macro2::Span> = None;

    for variant in variants {
        let ident = &variant.ident;
        let Some(code) = crate::fetch_code_from_attrs(&variant.attrs)? else {
            return Err(syn::Error::new(
                variant.span(),
                format!("variant `{ident}` is missing a #[code(\"...\")] attribute"),
            ));
        };

        let construct = build_construct(enum_name, variant)?;

        if code == "_" {
            if let Some(first) = catch_all_span {
                let mut error = syn::Error::new(variant.span(), "#[code(\"_\")] catch-all may appear only once");
                error.combine(syn::Error::new(first, "first catch-all is declared here"));
                return Err(error);
            }
            catch_all_span = Some(variant.span());
            catch_all_arm = Some(quote! { _ => #construct, });
        } else {
            concrete_arms.push(quote! { #code => #construct, });
        }
    }

    let Some(catch_all_arm) = catch_all_arm else {
        return Err(syn::Error::new_spanned(
            input,
            "#[derive(FromErrorMessage)] requires exactly one variant tagged #[code(\"_\")] as the catch-all",
        ));
    };

    let arms = quote! {
        #(#concrete_arms)*
        #catch_all_arm
    };

    let leto = crate::runtime_path();

    Ok(quote! {
        impl From<#leto::ErrorMessage> for #enum_name {
            #[track_caller]
            fn from(err: #leto::ErrorMessage) -> Self {
                match err.code.as_str() {
                    #arms
                }
            }
        }

        impl From<&#leto::ErrorMessage> for #enum_name {
            #[track_caller]
            fn from(err: &#leto::ErrorMessage) -> Self {
                match err.code.as_str() {
                    #arms
                }
            }
        }

        impl From<#leto::ApiError<&'static str>> for #enum_name {
            #[track_caller]
            fn from(err: #leto::ApiError) -> Self {
                match *err.code() {
                    #arms
                }
            }
        }

        impl From<&#leto::ApiError<&'static str>> for #enum_name {
            #[track_caller]
            fn from(err: &#leto::ApiError) -> Self {
                match *err.code() {
                    #arms
                }
            }
        }
    })
}

/// Build the variant construction that carries the wire error into the variant.
/// Every variant must hold exactly one field (the `ErrorMessage`): a tuple
/// `Variant(ErrorMessage)` or a single-field struct `Variant { field: ErrorMessage }`.
fn build_construct(enum_name: &syn::Ident, variant: &Variant) -> syn::Result<TokenStream2> {
    let ident = &variant.ident;
    let value = quote! { err.to_error_message().into() };
    match &variant.fields {
        Fields::Unnamed(fields) => {
            if fields.unnamed.len() != 1 {
                return Err(syn::Error::new(
                    variant.span(),
                    format!(
                        "#[derive(FromErrorMessage)] variant `{ident}` must hold exactly one field (the ErrorMessage)"
                    ),
                ));
            }
            Ok(quote! { #enum_name::#ident(#value) })
        }
        Fields::Named(fields) => {
            if fields.named.len() != 1 {
                return Err(syn::Error::new(
                    variant.span(),
                    format!(
                        "#[derive(FromErrorMessage)] struct variant `{ident}` must have exactly one field (the \
                         ErrorMessage)"
                    ),
                ));
            }
            let Some(field) = fields.named.first().and_then(|f| f.ident.as_ref()) else {
                return Err(syn::Error::new(
                    variant.span(),
                    format!("#[derive(FromErrorMessage)] struct variant `{ident}` must have a named field"),
                ));
            };
            Ok(quote! { #enum_name::#ident { #field: #value } })
        }
        Fields::Unit => {
            Err(syn::Error::new(
                variant.span(),
                format!(
                    "#[derive(FromErrorMessage)] variant `{ident}` must hold an ErrorMessage; unit variants are not \
                     supported"
                ),
            ))
        }
    }
}
