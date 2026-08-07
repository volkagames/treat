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

    let treat = crate::runtime_path();

    let mut from_api_err_impl: Vec<TokenStream> = Vec::with_capacity(variants.len());
    // Collected unconditionally, but only emitted when at least one variant
    // declares `#[status(...)]` — otherwise the enum keeps working with the
    // crate default instead of gaining a pointless all-default impl.
    let mut status_impl: Vec<TokenStream> = Vec::with_capacity(variants.len());
    let mut has_status = false;
    for variant in variants {
        let ident = &variant.ident;
        let message = crate::fetch_message_from_attrs(&variant.attrs)?;
        let fields_defs = variant_fields_defs(&variant.fields);
        let status = crate::fetch_status_from_attrs(&variant.attrs)?;
        let with_status = status.map(|status| quote! { .with_status(#status) });
        from_api_err_impl.push(match message {
            Some(message) => {
                match variant.fields {
                    Fields::Unit => {
                        quote! {
                            #enum_name::#ident => #treat::error(value).with_message(#message) #with_status,
                        }
                    }
                    Fields::Named(_) => {
                        quote! {
                            #enum_name::#ident {#fields_defs ..} => {
                                let message = format!(#message);
                                #treat::error(value).with_message(message) #with_status
                            },
                        }
                    }
                    Fields::Unnamed(_) => {
                        quote! {
                            #enum_name::#ident(#fields_defs ..) => {
                                let message = format!(#message);
                                #treat::error(value).with_message(message) #with_status
                            },
                        }
                    }
                }
            }
            None => {
                match variant.fields {
                    Fields::Unit => {
                        quote! {
                            #enum_name::#ident => #treat::error(value) #with_status,
                        }
                    }
                    Fields::Named(_) => {
                        quote! {
                            #enum_name::#ident { .. } => #treat::error(value) #with_status,
                        }
                    }
                    Fields::Unnamed(_) => {
                        quote! {
                            #enum_name::#ident( ..) => #treat::error(value) #with_status,
                        }
                    }
                }
            }
        });

        let status_arm = status.map_or_else(|| quote! { #treat::DEFAULT_ERROR_STATUS }, |status| quote! { #status });
        status_impl.push(match variant.fields {
            Fields::Unit => quote! { #enum_name::#ident => #status_arm, },
            Fields::Named(_) => quote! { #enum_name::#ident { .. } => #status_arm, },
            Fields::Unnamed(_) => quote! { #enum_name::#ident( .. ) => #status_arm, },
        });
        has_status |= status.is_some();
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

    let status_ts = has_status.then(|| {
        quote! {
            impl #treat::ApiErrorStatus for #enum_name {
                fn status_code(&self) -> u16 {
                    match self {
                        #(#status_impl)*
                    }
                }
            }
        }
    });

    let ts = quote! {
        impl From<#enum_name> for #treat::ApiError<#enum_name> {
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

        impl<T: #treat::ResponseData, M: #treat::ResponseData> From<#enum_name>
            for std::result::Result<#treat::ApiResponse<T, M>, #enum_name> {
            fn from(code: #enum_name) -> Self {
                Self::Err(code.into())
            }
        }

        #status_ts
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
