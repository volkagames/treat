use crate::{ApiError, ApiErrorCode};

/// An enum whose values travel by wire name.
///
/// Derive it with `#[derive(treat::rpc::WireEnum)]` on an enum of unit
/// variants (and `Copy`, which the trait requires: a code is a name, passed
/// around by value). A variant's wire name is its `#[code("...")]` (the
/// attribute [`ApiErrorCode`](crate::ApiErrorCode)'s derive reads for
/// `Display`), else its `#[serde(rename = "...")]` or the enum's
/// `#[serde(rename_all = "...")]`, else its identifier (a raw one unprefixed:
/// `r#type` is `type`). The derive refuses an enum where `#[code]` and a name
/// serde is told (`rename`, `rename_all`) disagree, and one serde would write
/// and read under different names.
///
/// `as_str` and `Display` from the `ApiErrorCode` derive always write one
/// string: that derive reads `#[code]`, serde's `rename` / `rename_all` and the
/// identifier in the same order. serde writes it too wherever serde is told a
/// name, or neither is: a `#[code]` serde is not told about is the wire name,
/// while serde still writes the identifier — add a matching
/// `#[serde(rename)]` if the enum also travels as a payload. serde's `alias`,
/// `other` and `skip_*` are not mirrored: `from_name` accepts the wire names
/// only, and `ALL` lists every variant.
///
/// The runtime reads codes by name instead of through serde because a name is
/// what crosses between code sets: a flat registry and the narrower set an
/// operation declares are different types that share wire names.
pub trait WireEnum: Sized + Copy + 'static {
    /// Every value, in declaration order.
    const ALL: &'static [Self];

    /// The value whose wire name is `name`; `None` for a name outside this enum.
    fn from_name(name: &str) -> Option<Self>;

    /// The wire name of this value: its `#[code]`, else the name serde writes.
    ///
    /// What serde writes only where serde is told the same name or neither
    /// is spelled: a variant with `#[code("things::gone")]` and no serde
    /// rename answers `"things::gone"` here, while serde still writes its
    /// identifier. The derive refuses only a `#[code]` and a serde name that
    /// disagree.
    ///
    /// By value: the type is `Copy`, and a call through a reference
    /// (`code.as_str()` on a `&R`) still works by auto-deref.
    fn as_str(self) -> &'static str;
}

/// The registry codes the runtime reports on its own, without a handler
/// choosing them: a request it could not read, and a failure of its own.
///
/// Implemented by the generated flat registry, so runtime helpers can build a
/// typed failure without the code names being templated into them.
pub trait RuntimeCodes: WireEnum + ApiErrorCode {
    /// A request body, query or path the runtime could not decode or validate.
    const INVALID_BODY: Self;
    /// A failure on the service's side, including a code a handler raised
    /// without declaring it.
    const INTERNAL_ERROR: Self;
}

/// What a set of codes answers with for a code it does not declare.
///
/// Derived by `#[derive(WireEnum)]` from the variant marked `#[undeclared]`,
/// normally the set's `internal_error`: a code the operation never promised is
/// the service's failure, not one the caller is expected to handle.
pub trait Undeclared: Sized {
    /// The value an undeclared code is reported as.
    const UNDECLARED: Self;
}

/// `meta` key carrying the code that was raised but never declared.
pub const UNDECLARED_CODE_META: &str = "undeclared_code";

/// The error every function below a handler answers with: the flat registry.
///
/// One type, so a check shared between operations composes with a plain `?` and
/// can be reused anywhere a value can go. The declared set is checked where it
/// is declared, one [`Narrow::narrow`] away.
pub type Failure<R> = ApiError<R>;

/// Report a flat [`Failure`] through the set the operation declares.
///
/// A declared code becomes that set's variant and nothing else changes. An
/// undeclared one is a gap in the operation's `@throws`: it is reported as the
/// set's [`Undeclared`] value with the raised code in `meta` under
/// [`UNDECLARED_CODE_META`], in every build: it is the service's failure, not
/// a reason to drop the request, and the `meta` entry is what finds the gap.
pub trait Narrow<T> {
    /// Map the error onto the code set `C` by wire name.
    fn narrow<C>(self) -> Result<T, ApiError<C>>
    where
        C: ApiErrorCode + WireEnum + Undeclared;
}

/// Any registry narrows: the bound is what narrowing reads — the wire name of
/// the raised code — not [`RuntimeCodes`], which only the runtime's own
/// failures need.
impl<T, R> Narrow<T> for Result<T, ApiError<R>>
where
    R: WireEnum + ApiErrorCode,
{
    fn narrow<C>(self) -> Result<T, ApiError<C>>
    where
        C: ApiErrorCode + WireEnum + Undeclared,
    {
        let failure = match self {
            Ok(value) => return Ok(value),
            Err(failure) => failure,
        };
        // By name rather than `TryFrom`: a blanket `TryFrom<R>` for every set is
        // refused by coherence, and the name is what the two types agree on
        // anyway.
        let raised = failure.code().as_str();
        let declared = C::from_name(raised);
        let narrowed = match declared {
            Some(declared) => failure.map_code(|_| declared),
            None => {
                // The raised code is merged into whatever diagnostics the check
                // below already attached, because this branch is the untested
                // path and its `meta` is what says why.
                let mut meta = failure.meta().cloned().unwrap_or_default();
                meta.insert(
                    UNDECLARED_CODE_META.to_string(),
                    serde_json::Value::String(raised.to_string()),
                );
                failure
                    .map_code(|_| C::UNDECLARED)
                    .with_meta(serde_json::Value::Object(meta))
            }
        };
        Err(narrowed)
    }
}
