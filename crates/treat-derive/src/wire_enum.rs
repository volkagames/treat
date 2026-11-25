use proc_macro2::{Span, TokenStream};
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

/// The name one half of serde gives a variant, where serde was told it.
struct SerdeName {
    name: String,
    span: Span,
    source: &'static str,
}

/// One half's name: the variant's own `rename`, else the enum's `rename_all`
/// rule applied to the (unraw) identifier; `None` when serde was told neither
/// and writes the identifier.
fn serde_half(renamed: Option<LitStr>, rename_all: Option<&(LitStr, RenameRule)>, ident: &str) -> Option<SerdeName> {
    match (renamed, rename_all) {
        (Some(lit), _) => {
            Some(SerdeName {
                name: lit.value(),
                span: lit.span(),
                source: "#[serde(rename)]",
            })
        }
        (None, Some((lit, rule))) => {
            Some(SerdeName {
                name: rule.apply(ident),
                span: lit.span(),
                source: "#[serde(rename_all)]",
            })
        }
        (None, None) => None,
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

/// The `serialize` and `deserialize` halves of a serde key; `key = "..."`
/// sets both.
struct Halves<T> {
    serialize: Option<T>,
    deserialize: Option<T>,
}

/// The enum's `#[serde(rename_all = "...")]`, per half, with the rule each
/// half names.
fn serde_rename_all(attrs: &[syn::Attribute]) -> syn::Result<Halves<(LitStr, RenameRule)>> {
    let lits = serde_key(attrs, "rename_all")?;
    Ok(Halves {
        serialize: lits.serialize.map(with_rule).transpose()?,
        deserialize: lits.deserialize.map(with_rule).transpose()?,
    })
}

/// A `rename_all` value with the rule it names; an unknown rule is an error
/// here rather than a silently wrong name, though serde would refuse it too.
fn with_rule(lit: LitStr) -> syn::Result<(LitStr, RenameRule)> {
    let rule = RenameRule::from_str(&lit.value())
        .ok_or_else(|| syn::Error::new(lit.span(), format!("unknown serde rename rule `{}`", lit.value())))?;
    Ok((lit, rule))
}

/// The halves a `#[serde(key = "...")]` or `#[serde(key(serialize = "...",
/// deserialize = "..."))]` gives, across every `#[serde]` attribute. Other
/// serde keys (`alias`, `other`, `skip_*`, ...) are skipped unread: serde
/// validates them itself, and none of them changes the one name a variant is
/// written with.
fn serde_key(attrs: &[syn::Attribute], key: &str) -> syn::Result<Halves<LitStr>> {
    let mut found = Halves {
        serialize: None,
        deserialize: None,
    };
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident(key) {
                return skip_value(&meta);
            }
            if meta.input.peek(syn::Token![=]) {
                let value = meta.value()?;
                let lit: LitStr = value.parse()?;
                found.serialize = Some(lit.clone());
                found.deserialize = Some(lit);
                return Ok(());
            }
            meta.parse_nested_meta(|half| {
                let value = half.value()?;
                let lit: LitStr = value.parse()?;
                if half.path.is_ident("serialize") {
                    found.serialize = Some(lit);
                } else if half.path.is_ident("deserialize") {
                    found.deserialize = Some(lit);
                }
                Ok(())
            })
        })?;
    }
    Ok(found)
}

/// Consume the value of a serde key this derive does not read, up to the next
/// comma: `= "..."`, a parenthesised list, or nothing for a bare flag.
fn skip_value(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    let input = meta.input;
    while !input.is_empty() && !input.peek(syn::Token![,]) {
        input.parse::<proc_macro2::TokenTree>()?;
    }
    Ok(())
}

/// serde's `rename_all` rules, applied to a variant the way serde applies them.
#[derive(Clone, Copy)]
enum RenameRule {
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
    ScreamingKebab,
}

impl RenameRule {
    fn from_str(rule: &str) -> Option<Self> {
        Some(match rule {
            "lowercase" => Self::Lower,
            "UPPERCASE" => Self::Upper,
            "PascalCase" => Self::Pascal,
            "camelCase" => Self::Camel,
            "snake_case" => Self::Snake,
            "SCREAMING_SNAKE_CASE" => Self::ScreamingSnake,
            "kebab-case" => Self::Kebab,
            "SCREAMING-KEBAB-CASE" => Self::ScreamingKebab,
            _ => return None,
        })
    }

    /// The name serde gives the variant `ident` under this rule (serde's
    /// `RenameRule::apply_to_variant`).
    fn apply(self, ident: &str) -> String {
        match self {
            Self::Pascal => ident.to_string(),
            Self::Lower => ident.to_ascii_lowercase(),
            Self::Upper => ident.to_ascii_uppercase(),
            Self::Camel => {
                let mut chars = ident.chars();
                let first = chars.next().map(|first| first.to_ascii_lowercase());
                first.into_iter().chain(chars).collect()
            }
            Self::Snake => {
                let mut snake = String::with_capacity(ident.len());
                for (index, ch) in ident.char_indices() {
                    if index > 0 && ch.is_uppercase() {
                        snake.push('_');
                    }
                    snake.push(ch.to_ascii_lowercase());
                }
                snake
            }
            Self::ScreamingSnake => Self::Snake.apply(ident).to_ascii_uppercase(),
            Self::Kebab => Self::Snake.apply(ident).replace('_', "-"),
            Self::ScreamingKebab => Self::ScreamingSnake.apply(ident).replace('_', "-"),
        }
    }
}
