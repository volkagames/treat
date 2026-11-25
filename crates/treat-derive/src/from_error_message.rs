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
    // Each concrete code with the variant that claimed it first. A second
    // variant with the same code would be an unreachable arm, and rustc
    // suppresses `unreachable_patterns` on macro spans, so it is refused here.
    let mut seen_codes: Vec<(String, &syn::Ident)> = Vec::new();
    let mut duplicates: Option<syn::Error> = None;

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
            let first = seen_codes
                .iter()
                .find(|(seen, _)| *seen == code)
                .map(|(_, first)| *first);
            match first {
                Some(first) => {
                    let mut error = syn::Error::new(
                        variant.span(),
                        format!("#[code(\"{code}\")] is already used by `{first}`; each code may map to one variant"),
                    );
                    error.combine(syn::Error::new(
                        first.span(),
                        format!("`{first}` declares #[code(\"{code}\")] here"),
                    ));
                    match duplicates.as_mut() {
                        Some(errors) => errors.combine(error),
                        None => duplicates = Some(error),
                    }
                }
                None => {
                    seen_codes.push((code.clone(), ident));
                    concrete_arms.push(quote! { #code => #construct, });
                }
            }
        }
    }
    if let Some(errors) = duplicates {
        return Err(errors);
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

    let treat = crate::runtime_path();

    // A generic enum keeps its own bounds; each variant's field is built with
    // `Into::into(ErrorMessage)`, so a field whose type is generic must be one
    // an `ErrorMessage` converts into. Only a generic enum gets the bounds, so a
    // concrete one keeps its errors where they were.
    let generics = match input.generics.params.is_empty() {
        true => input.generics.clone(),
        false => {
            let field_types = variants
                .iter()
                .flat_map(|variant| variant.fields.iter())
                .map(|field| &field.ty);
            let bounds = field_types.map(|ty| -> syn::WherePredicate {
                syn::parse_quote! { #ty: ::core::convert::From<#treat::ErrorMessage> }
            });
            crate::with_predicates(&input.generics, bounds)
        }
    };
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let this = quote! { #enum_name #ty_generics };
    let api_error = quote! { #treat::ApiError<&'static ::core::primitive::str> };

    Ok(quote! {
        impl #impl_generics ::core::convert::From<#treat::ErrorMessage> for #this #where_clause {
            #[track_caller]
            fn from(err: #treat::ErrorMessage) -> Self {
                match err.code.as_str() {
                    #arms
                }
            }
        }

        impl #impl_generics ::core::convert::From<&#treat::ErrorMessage> for #this #where_clause {
            #[track_caller]
            fn from(err: &#treat::ErrorMessage) -> Self {
                match err.code.as_str() {
                    #arms
                }
            }
        }

        impl #impl_generics ::core::convert::From<#api_error> for #this #where_clause {
            #[track_caller]
            fn from(err: #api_error) -> Self {
                match *err.code() {
                    #arms
                }
            }
        }

        impl #impl_generics ::core::convert::From<&#api_error> for #this #where_clause {
            #[track_caller]
            fn from(err: &#api_error) -> Self {
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
    let value = quote! { ::core::convert::Into::into(err.to_error_message()) };
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
