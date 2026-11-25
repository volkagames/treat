use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::Data::Enum;
use syn::ext::IdentExt;
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
    // The conversion's argument and the formatted message are bound at
    // `mixed_site`, so a variant field named `value` or `message` (bound by the
    // arm's pattern for `#[message]` interpolation) cannot capture them.
    let value = syn::Ident::new("value", Span::mixed_site());
    let message_var = syn::Ident::new("message", Span::mixed_site());

    let mut from_api_err_impl: Vec<TokenStream> = Vec::with_capacity(variants.len());
    // Collected unconditionally, but only emitted when at least one variant
    // declares `#[status(...)]` — otherwise the enum keeps working with the
    // crate default instead of gaining a pointless all-default impl.
    let mut status_impl: Vec<TokenStream> = Vec::with_capacity(variants.len());
    let mut has_status = false;
    for variant in variants {
        let ident = &variant.ident;
        let message = crate::fetch_message_from_attrs(&variant.attrs)?;
        let status = crate::fetch_status_from_attrs(&variant.attrs)?;
        let with_status = status.map(|status| quote! { .with_status(#status) });
        from_api_err_impl.push(match message {
            Some(message) => {
                // Bind only the fields the message names: a field bound and
                // never read is an `unused_variables` warning in the user's
                // crate, which `-D warnings` turns into a failed build.
                let referenced = format_arg_names(&message);
                let fields_defs = variant_fields_defs(&variant.fields, &referenced);
                match variant.fields {
                    Fields::Unit => {
                        quote! {
                            #enum_name::#ident => #treat::error(#value).with_message(#message) #with_status,
                        }
                    }
                    Fields::Named(_) => {
                        quote! {
                            #enum_name::#ident {#fields_defs ..} => {
                                let #message_var = ::std::format!(#message);
                                #treat::error(#value).with_message(#message_var) #with_status
                            },
                        }
                    }
                    Fields::Unnamed(_) => {
                        quote! {
                            #enum_name::#ident(#fields_defs ..) => {
                                let #message_var = ::std::format!(#message);
                                #treat::error(#value).with_message(#message_var) #with_status
                            },
                        }
                    }
                }
            }
            None => {
                match variant.fields {
                    Fields::Unit => {
                        quote! {
                            #enum_name::#ident => #treat::error(#value) #with_status,
                        }
                    }
                    Fields::Named(_) => {
                        quote! {
                            #enum_name::#ident { .. } => #treat::error(#value) #with_status,
                        }
                    }
                    Fields::Unnamed(_) => {
                        quote! {
                            #enum_name::#ident( ..) => #treat::error(#value) #with_status,
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

    // A variant's code is its `#[code]`, else the name serde writes for it,
    // else its identifier (a raw one unprefixed: `r#type` is `type`) — the
    // order `WireEnum` reads too, so `Display`, `as_str` and serde agree on a
    // variant whichever of them spells its name.
    let rename_all = crate::serde_name::serde_rename_all(&input.attrs)?;
    let mut display_impl: Vec<TokenStream> = Vec::with_capacity(variants.len());
    for variant in variants {
        let ident = &variant.ident;
        let code = match crate::fetch_code_from_attrs(&variant.attrs)? {
            Some(code) => code,
            None => {
                let written = crate::serde_name::serde_written_name(variant, &rename_all)?;
                written.unwrap_or_else(|| ident.unraw().to_string())
            }
        };
        display_impl.push(match variant.fields {
            Fields::Unit => {
                quote! {
                    #enum_name::#ident => ::core::write!(f, "{}", #code),
                }
            }
            Fields::Named(_) => {
                quote! {
                    #enum_name::#ident { .. } => ::core::write!(f, "{}", #code),
                }
            }
            Fields::Unnamed(_) => {
                quote! {
                    #enum_name::#ident( .. ) => ::core::write!(f, "{}", #code),
                }
            }
        });
    }

    // A generic enum is a code only where its parameters make it one: the
    // conversion into `ApiError<Self>` is bounded on `Self` being a `'static`
    // `ApiErrorCode`, so the impl exists exactly for the instantiations that
    // satisfy it and the enum's own bounds carry over unchanged.
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let this = quote! { #enum_name #ty_generics };
    let code_generics = crate::with_predicates(
        &input.generics,
        [syn::parse_quote! { #this: #treat::ApiErrorCode + 'static }],
    );
    let (code_impl_generics, _, code_where_clause) = code_generics.split_for_impl();

    let status_ts = has_status.then(|| {
        quote! {
            impl #impl_generics #treat::ApiErrorStatus for #this #where_clause {
                fn status_code(&self) -> ::core::primitive::u16 {
                    match self {
                        #(#status_impl)*
                    }
                }
            }
        }
    });

    // The parameters are named apart from anything a user would call an enum
    // or a type parameter, so an enum named `T` or `M` is not captured by them.
    let result_generics = crate::with_type_params(
        &code_generics,
        [
            syn::parse_quote! { __TreatData: #treat::ResponseData },
            syn::parse_quote! { __TreatMeta: #treat::ResponseData },
        ],
    );
    let (result_impl_generics, _, result_where_clause) = result_generics.split_for_impl();

    let ts = quote! {
        impl #code_impl_generics ::core::convert::From<#this> for #treat::ApiError<#this> #code_where_clause {
            #[track_caller]
            fn from(#value: #this) -> Self {
                match &#value {
                    #(#from_api_err_impl)*
                }
            }
        }

        impl #impl_generics ::core::fmt::Display for #this #where_clause {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    #(#display_impl)*
                }
            }
        }

        impl #result_impl_generics ::core::convert::From<#this>
            for ::core::result::Result<#treat::ApiResponse<__TreatData, __TreatMeta>, #this> #result_where_clause {
            fn from(code: #this) -> Self {
                ::core::result::Result::Err(code)
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

/// The pattern bindings for a variant's fields, binding only the ones in
/// `referenced` (the names a `#[message]` format string reads). Tuple fields are
/// bound as `arg_0`, `arg_1`, ...; an unread tuple field is `_`. The caller
/// closes the pattern with `..`.
fn variant_fields_defs(fields: &syn::Fields, referenced: &[String]) -> TokenStream {
    let is_referenced = |name: &str| referenced.iter().any(|r| r == name);
    match fields {
        syn::Fields::Unit => {
            quote! {}
        }
        syn::Fields::Named(fields) => {
            let recurse = fields.named.iter().filter_map(|f| {
                let name = f.ident.as_ref()?;
                // A raw identifier is read under its unprefixed name too.
                let read = is_referenced(&name.to_string()) || is_referenced(&name.unraw().to_string());
                read.then(|| quote! { #name, })
            });
            quote! {#(#recurse)*}
        }
        syn::Fields::Unnamed(fields) => {
            let recurse = fields.unnamed.iter().enumerate().map(|(index, _f)| {
                let name = format_ident!("arg_{index}");
                match is_referenced(&name.to_string()) {
                    true => quote! { #name, },
                    false => quote! { _, },
                }
            });

            quote! {#(#recurse)*}
        }
    }
}

/// The identifiers a format string reads: each `{name}` / `{name:spec}`
/// argument and each `name$` width or precision inside a spec. `{{` and `}}` are
/// escapes. Positional arguments (`{0}`, `{}`) name nothing.
///
/// An over-approximation is harmless (the field is bound and read anyway, or
/// the name is no field), so anything identifier-shaped counts.
fn format_arg_names(format: &str) -> Vec<String> {
    // Dropping the escapes first leaves every `{` opening an argument. For a
    // valid format string this keeps each argument's own braces: in `{a}}}`
    // the leftmost `}}` is removed, and the `}` left over still closes `{a`.
    let unescaped = format.replace("{{", "").replace("}}", "");
    let mut names = Vec::new();
    for piece in unescaped.split('{').skip(1) {
        let spec = piece.split_once('}').map_or(piece, |(spec, _)| spec);
        let (argument, format_spec) = spec.split_once(':').unwrap_or((spec, ""));
        push_identifier(&mut names, argument.trim());
        // `name$` in a width or precision; a fill character is always followed
        // by an alignment, never by `$`.
        let mut word = String::new();
        for c in format_spec.chars() {
            if c == '$' {
                push_identifier(&mut names, &word);
            }
            match c.is_alphanumeric() || c == '_' {
                true => word.push(c),
                false => word.clear(),
            }
        }
    }
    names
}

fn push_identifier(names: &mut Vec<String>, candidate: &str) {
    let candidate = candidate.strip_prefix("r#").unwrap_or(candidate);
    let starts_like_identifier = candidate.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_');
    if starts_like_identifier && candidate != "_" {
        names.push(candidate.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::format_arg_names;

    #[test]
    fn format_arg_names_reads_arguments_and_counts() {
        assert_eq!(format_arg_names("only {a}"), ["a"]);
        assert_eq!(
            format_arg_names("{a:?} and {b:>width$.prec$}"),
            ["a", "b", "width", "prec"]
        );
        assert_eq!(format_arg_names("{{escaped}} {arg_0} {} {0}"), ["arg_0"]);
        assert_eq!(format_arg_names("{{{a}}}"), ["a"]);
        assert!(format_arg_names("plain").is_empty());
    }
}
