use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Data::Enum;
use syn::{DataEnum, DeriveInput, Fields};

pub fn derive(input: &DeriveInput) -> syn::Result<TokenStream> {
    let enum_name = &input.ident;

    let Enum(DataEnum { ref variants, .. }) = input.data else {
        return Err(syn::Error::new_spanned(
            input,
            "#[derive(ApiErrorCode)] support only enums",
        ));
    };

    let leto = crate::runtime_path();

    let mut from_api_err_impl: Vec<TokenStream> = Vec::with_capacity(variants.len());
    for variant in variants {
        let ident = &variant.ident;
        let message = crate::fetch_message_from_attrs(&variant.attrs)?;
        let fields_defs = variant_fields_defs(&variant.fields);
        from_api_err_impl.push(match message {
            Some(message) => {
                match variant.fields {
                    Fields::Unit => {
                        quote! {
                            #enum_name::#ident => #leto::error(value).with_message(#message),
                        }
                    }
                    Fields::Named(_) => {
                        quote! {
                            #enum_name::#ident {#fields_defs ..} => {
                                let message = format!(#message);
                                #leto::error(value).with_message(message)
                            },
                        }
                    }
                    Fields::Unnamed(_) => {
                        quote! {
                            #enum_name::#ident(#fields_defs ..) => {
                                let message = format!(#message);
                                #leto::error(value).with_message(message)
                            },
                        }
                    }
                }
            }
            None => {
                match variant.fields {
                    Fields::Unit => {
                        quote! {
                            #enum_name::#ident => #leto::error(value),
                        }
                    }
                    Fields::Named(_) => {
                        quote! {
                            #enum_name::#ident { .. } => #leto::error(value),
                        }
                    }
                    Fields::Unnamed(_) => {
                        quote! {
                            #enum_name::#ident( ..) => #leto::error(value),
                        }
                    }
                }
            }
        });
    }

    let mut display_impl: Vec<TokenStream> = Vec::with_capacity(variants.len());
    for variant in variants {
        let ident = &variant.ident;
        let code = crate::fetch_code_from_attrs(&variant.attrs)?.unwrap_or_else(|| ident.to_string());
        display_impl.push(match variant.fields {
            Fields::Unit => {
                quote! {
                    #enum_name::#ident =>  write!(f, "{}", #code),
                }
            }
            Fields::Named(_) => {
                quote! {
                    #enum_name::#ident { .. } =>  write!(f, "{}", #code),
                }
            }
            Fields::Unnamed(_) => {
                quote! {
                    #enum_name::#ident( .. ) =>  write!(f, "{}", #code),
                }
            }
        });
    }

    let ts = quote! {
        impl From<#enum_name> for #leto::ApiError<#enum_name> {
            #[track_caller]
            fn from(value: #enum_name) -> Self {
                match &value {
                    #(#from_api_err_impl)*
                }
            }
        }

        impl std::fmt::Display for #enum_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    #(#display_impl)*
                }
            }
        }

        impl<T: #leto::ResponseData, M: #leto::ResponseData> From<#enum_name>
            for std::result::Result<#leto::ApiResponse<T, M>, #enum_name> {
            fn from(code: #enum_name) -> Self {
                Self::Err(code.into())
            }
        }
    };

    // {
    //     use quote::ToTokens;
    //     println!("{}", ts.to_token_stream().to_string());
    // }

    Ok(ts)
}

fn variant_fields_defs(fields: &syn::Fields) -> TokenStream {
    match fields {
        syn::Fields::Unit => {
            quote! {}
        }
        syn::Fields::Named(fields) => {
            let recurse = fields.named.iter().filter_map(|f| {
                let name = f.ident.as_ref()?;
                Some(quote! { #name, })
            });
            quote! {#(#recurse)*}
        }
        syn::Fields::Unnamed(fields) => {
            let recurse = fields.unnamed.iter().enumerate().map(|(index, _f)| {
                let name = format_ident!("arg_{index}");
                quote! { #name, }
            });

            quote! {#(#recurse)*}
        }
    }
}
