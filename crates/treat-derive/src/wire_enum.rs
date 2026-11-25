use crate::serde_name::{Halves, RenameRule, serde_half, serde_key, serde_rename_all};
use proc_macro2::TokenStream;
use quote::quote;
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, LitStr, Variant};

pub fn derive(input: &DeriveInput) -> syn::Result<TokenStream> {
    let enum_name = &input.ident;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "#[derive(WireEnum)] supports only enums of unit variants",
        ));
    };
    // A wire enum names a fixed set of values; a generic one would need bounds
    // the derive cannot guess, and has no use the runtime knows of.
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "#[derive(WireEnum)] does not support generic enums",
        ));
    }

    let rename_all = serde_rename_all(&input.attrs)?;
    let mut idents = Vec::with_capacity(data.variants.len());
    let mut wires: Vec<String> = Vec::with_capacity(data.variants.len());
    let mut undeclared: Option<&Variant> = None;
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                &variant.fields,
                "#[derive(WireEnum)] supports only unit variants: a wire name carries no data",
            ));
        }
        let wire = wire_name(variant, &rename_all)?;
        // Two variants with one name would make `from_name` pick the first
        // silently and `as_str` stop round-tripping.
        let clash = wires.iter().position(|seen| *seen == wire);
        if let Some(index) = clash {
            let other = idents.get(index).map(ToString::to_string).unwrap_or_default();
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!("wire name `{wire}` is already used by `{other}`"),
            ));
        }
        let marker = undeclared_marker(variant)?;
        if let Some(marker) = marker {
            if let Some(first) = undeclared {
                return Err(syn::Error::new(
                    marker.span(),
                    format!(
                        "`#[undeclared]` is already on `{}`: a set answers an undeclared code with one value",
                        first.ident
                    ),
                ));
            }
            undeclared = Some(variant);
        }
        idents.push(&variant.ident);
        wires.push(wire);
    }

    let treat = crate::runtime_path();
    let undeclared = undeclared.map(|variant| {
        let ident = &variant.ident;
        quote! {
            #[automatically_derived]
            impl #treat::rpc::Undeclared for #enum_name {
                const UNDECLARED: Self = Self::#ident;
            }
        }
    });

    Ok(quote! {
        #[automatically_derived]
        impl #treat::rpc::WireEnum for #enum_name {
            const ALL: &'static [Self] = &[#(Self::#idents),*];

            fn from_name(name: &::core::primitive::str) -> ::core::option::Option<Self> {
                match name {
                    #(#wires => ::core::option::Option::Some(Self::#idents),)*
                    _ => ::core::option::Option::None,
                }
            }

            fn as_str(self) -> &'static ::core::primitive::str {
                match self {
                    #(Self::#idents => #wires,)*
                }
            }
        }

        #undeclared
    })
}

/// The wire name of a variant: `#[code]`, else the name serde writes, else the
/// identifier — raw identifiers unprefixed (`r#type` is `type`), as serde and
/// `ApiErrorCode`'s `Display` spell them.
///
/// `#[code]` is what `ApiErrorCode` writes as `Display`, serde's name is what
/// goes into a JSON body; when both are spelled they must agree, or the same
/// value would read differently in an error envelope and in a payload.
///
/// serde's name is worked out per half: a `serialize` and a `deserialize` half
/// of `rename` or `rename_all` that name different strings are refused only
/// for a variant they actually give two names — one with its own `rename`
/// overrides the enum's `rename_all`, so differing `rename_all` halves are
/// harmless while every variant spells its name.
fn wire_name(variant: &Variant, rename_all: &Halves<(LitStr, RenameRule)>) -> syn::Result<String> {
    let code = crate::fetch_code_from_attrs(&variant.attrs)?;
    let renamed = serde_key(&variant.attrs, "rename")?;
    let ident = variant.ident.unraw().to_string();
    let serialize = serde_half(renamed.serialize, rename_all.serialize.as_ref(), &ident);
    let deserialize = serde_half(renamed.deserialize, rename_all.deserialize.as_ref(), &ident);
    // serde's own name for the variant, with where it came from for the error;
    // `None` when neither half is spelled and serde writes the identifier.
    // A half serde was told nothing for writes the identifier, so a lone
    // `serialize` half naming the identifier is one name too.
    let written = serialize.as_ref().map_or(ident.as_str(), |half| half.name.as_str());
    let read = deserialize.as_ref().map_or(ident.as_str(), |half| half.name.as_str());
    if written != read {
        let blame = serialize.as_ref().or(deserialize.as_ref());
        let span = blame.map_or_else(|| variant.ident.span(), |half| half.span);
        return Err(syn::Error::new(
            span,
            format!(
                "`{}`: serde writes \"{written}\" but reads \"{read}\"; #[derive(WireEnum)] needs one wire name, give \
                 `serialize` and `deserialize` the same value or rename the variant itself",
                variant.ident
            ),
        ));
    }
    // serde's own name for the variant, with where it came from for the error;
    // `None` when neither half is spelled and serde writes the identifier.
    let serde_name = serialize.or(deserialize);
    match (code, serde_name) {
        (Some(code), Some(serde_name)) if code != serde_name.name => {
            Err(syn::Error::new(
                serde_name.span,
                format!(
                    "`{}` has #[code(\"{code}\")] but {} makes serde write \"{}\"; a wire name must be one string, \
                     spell the same one in both",
                    variant.ident, serde_name.source, serde_name.name
                ),
            ))
        }
        (Some(code), _) => Ok(code),
        (None, Some(serde_name)) => Ok(serde_name.name),
        (None, None) => Ok(ident),
    }
}

/// The `#[undeclared]` attribute of a variant, if it has one.
fn undeclared_marker(variant: &Variant) -> syn::Result<Option<&syn::Attribute>> {
    let attr = variant.attrs.iter().find(|attr| attr.path().is_ident("undeclared"));
    if let Some(attr) = attr {
        attr.meta.require_path_only().map_err(|err| {
            syn::Error::new(
                err.span(),
                "`#[undeclared]` takes no arguments; it marks the variant an undeclared code becomes",
            )
        })?;
    }
    Ok(attr)
}
