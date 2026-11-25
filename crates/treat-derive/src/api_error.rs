use proc_macro2::TokenStream;
use quote::quote;
use syn::Data::Enum;
use syn::spanned::Spanned;
use syn::{DataEnum, DeriveInput, Fields};

pub fn derive(input: &DeriveInput) -> syn::Result<TokenStream> {
    let enum_name = &input.ident;

    let Enum(DataEnum { ref variants, .. }) = input.data else {
        return Err(syn::Error::new_spanned(input, "#[derive(ApiError)] support only enums"));
    };

    let treat = crate::runtime_path();

    let mut inner_impl: Vec<TokenStream> = Vec::with_capacity(variants.len());
    // A variant without `#[code(...)]` would leave the generated `match`
    // non-exhaustive, which rustc reports as a cryptic E0004 inside the macro;
    // name every such variant instead.
    let mut missing_code: Option<syn::Error> = None;
    for variant in variants {
        let ident = &variant.ident;
        let Some(code) = crate::fetch_code_from_attrs(&variant.attrs)? else {
            let error = syn::Error::new(
                variant.span(),
                format!("#[derive(ApiError)] variant `{ident}` is missing a #[code(\"...\")] attribute"),
            );
            match missing_code.as_mut() {
                Some(errors) => errors.combine(error),
                None => missing_code = Some(error),
            }
            continue;
        };
        let status = crate::fetch_status_from_attrs(&variant.attrs)?.map(|status| quote! { .with_status(#status) });
        inner_impl.push(match variant.fields {
            Fields::Unit => {
                quote! {
                    #enum_name::#ident => #treat::error(#code).with_message(message) #status,
                }
            }
            Fields::Unnamed(_) => {
                quote! {
                    #enum_name::#ident(..) => #treat::error(#code).with_message(message) #status,
                }
            }
            Fields::Named(_) => {
                quote! {
                    #enum_name::#ident { .. } => #treat::error(#code).with_message(message) #status,
                }
            }
        });
    }
    if let Some(errors) = missing_code {
        return Err(errors);
    }

    fn has_attr(attrs: &[syn::Attribute], name: &str) -> bool {
        attrs.iter().any(|attr| attr.path().is_ident(name))
    }

    // A generic enum keeps its own bounds; the conversion additionally needs
    // `Display` for the message, which a `thiserror` impl may grant only under
    // bounds of its own, so it is required of `Self` rather than assumed.
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let this = quote! { #enum_name #ty_generics };
    let display_generics = crate::with_predicates(&input.generics, [syn::parse_quote! { #this: ::core::fmt::Display }]);
    let (display_impl_generics, _, display_where_clause) = display_generics.split_for_impl();

    let catch_all_variants = variants
        .iter()
        .filter(|variant| has_attr(&variant.attrs, "catch_all"))
        .collect::<Vec<_>>();

    let catch_all_impl = catch_all_variants
        .split_first()
        .map(|(variant, rest)| {
            if !rest.is_empty() {
                let message = "#[catch_all] may be used at most once";
                let mut error = syn::Error::new(variant.span(), message);
                for variant in rest {
                    error.combine(syn::Error::new(variant.span(), message));
                }
                return Err(error);
            }

            let ident = &variant.ident;
            // The generated `From<Report>` builds `Variant(report)`, so anything
            // but a one-field tuple fails inside the macro output instead.
            let single_tuple_field = matches!(&variant.fields, Fields::Unnamed(fields) if fields.unnamed.len() == 1);
            if !single_tuple_field {
                return Err(syn::Error::new(
                    ident.span(),
                    format!("#[catch_all] variant must hold exactly one field, in the form {ident}(erris::Report)"),
                ));
            }

            Ok(quote! {
                impl #impl_generics ::core::convert::From<#treat::erris::Report> for #this #where_clause {
                    #[track_caller]
                    fn from(value: #treat::erris::Report) -> Self {
                        #enum_name::#ident(value)
                    }
                }
            })
        })
        .transpose()?;

    let api_error = quote! { #treat::ApiError<&'static ::core::primitive::str> };

    Ok(quote! {
        impl #display_impl_generics ::core::convert::From<#this> for #api_error #display_where_clause {
            #[track_caller]
            fn from(value: #this) -> Self {
                let message = ::std::string::ToString::to_string(&value);
                match value {
                    #(#inner_impl)*
                }
            }
        }

        #catch_all_impl
    })
}
