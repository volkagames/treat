//! The names serde gives an enum's variants, read from `#[serde(rename)]` and
//! `#[serde(rename_all)]` the way serde reads them: what `WireEnum` and
//! `ApiErrorCode` both spell a variant with when it has no `#[code]`.

use proc_macro2::Span;
use syn::ext::IdentExt;
use syn::{LitStr, Variant};

/// The name serde writes for `variant` — the `serialize` half, which is what
/// goes on the wire: its own `rename`, else the enum's `rename_all` applied to
/// the (unraw) identifier; `None` when serde was told neither and writes the
/// identifier.
pub(crate) fn serde_written_name(
    variant: &Variant,
    rename_all: &Halves<(LitStr, RenameRule)>,
) -> syn::Result<Option<String>> {
    let renamed = serde_key(&variant.attrs, "rename")?;
    let ident = variant.ident.unraw().to_string();
    let written = serde_half(renamed.serialize, rename_all.serialize.as_ref(), &ident);
    Ok(written.map(|half| half.name))
}

/// The name one half of serde gives a variant, where serde was told it.
pub(crate) struct SerdeName {
    pub(crate) name: String,
    pub(crate) span: Span,
    pub(crate) source: &'static str,
}

/// One half's name: the variant's own `rename`, else the enum's `rename_all`
/// rule applied to the (unraw) identifier; `None` when serde was told neither
/// and writes the identifier.
pub(crate) fn serde_half(
    renamed: Option<LitStr>,
    rename_all: Option<&(LitStr, RenameRule)>,
    ident: &str,
) -> Option<SerdeName> {
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

/// The `serialize` and `deserialize` halves of a serde key; `key = "..."`
/// sets both.
pub(crate) struct Halves<T> {
    pub(crate) serialize: Option<T>,
    pub(crate) deserialize: Option<T>,
}

/// The enum's `#[serde(rename_all = "...")]`, per half, with the rule each
/// half names.
pub(crate) fn serde_rename_all(attrs: &[syn::Attribute]) -> syn::Result<Halves<(LitStr, RenameRule)>> {
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
pub(crate) fn serde_key(attrs: &[syn::Attribute], key: &str) -> syn::Result<Halves<LitStr>> {
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
pub(crate) enum RenameRule {
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
