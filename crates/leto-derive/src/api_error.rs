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

    let leto = crate::runtime_path();

    let mut inner_impl: Vec<TokenStream> = Vec::with_capacity(variants.len());
    for variant in variants {
        let ident = &variant.ident;
        // Variants without `#[code(...)]` contribute no match arm, matching the
        // previous behaviour.
        let Some(code) = crate::fetch_code_from_attrs(&variant.attrs)? else {
            continue;
        };
        inner_impl.push(match variant.fields {
            Fields::Unit => {
                quote! {
                    #enum_name::#ident => #leto::error(#code).with_message(message),
                }
            }
            Fields::Unnamed(_) => {
                quote! {
                    #enum_name::#ident(_) => #leto::error(#code).with_message(message),
                }
            }
            Fields::Named(_) => {
                quote! {
                    #enum_name::#ident { .. } => #leto::error(#code).with_message(message),
                }
            }
        });
    }

    fn has_attr(attrs: &[syn::Attribute], name: &str) -> bool {
        attrs.iter().any(|attr| attr.path().is_ident(name))
    }

    let catch_all_variants = variants
        .iter()
        .filter(|variant| has_attr(&variant.attrs, "catch_all"))
        .collect::<Vec<_>>();

    let catch_all_impl = catch_all_variants
        .split_first()
        .map(|(variant, rest)| {
            if !rest.is_empty() {
                let message = "[catch_all] attribute must be used exactly once";
                let mut error = syn::Error::new(variant.span(), message);
                for variant in rest {
                    error.combine(syn::Error::new(variant.span(), message));
                }
                return Err(error);
            }

            let ident = &variant.ident;
            if !matches!(variant.fields, Fields::Unnamed(_)) {
                return Err(syn::Error::new(
                    ident.span(),
                    format!("catch_all variant must be in form of {ident}(erris::Report)"),
                ));
            }

            Ok(quote! {
                impl From<#leto::erris::Report> for #enum_name {
                    #[track_caller]
                    fn from(value: #leto::erris::Report) -> Self {
                        #enum_name::#ident(value)
                    }
                }
            })
        })
        .transpose()?;

    Ok(quote! {
        impl From<#enum_name> for #leto::ApiError<&'static str> {
            #[track_caller]
            fn from(value: #enum_name) -> Self {
                let message = value.to_string();
                match value {
                    #(#inner_impl)*
                }
            }
        }

        #catch_all_impl
    })
}
