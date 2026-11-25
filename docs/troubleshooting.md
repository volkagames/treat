# Troubleshooting

Common compile errors and gotchas, with fixes.

## "cannot find derive macro `ApiError` / `ApiErrorCode` in this scope"

Bring the derives into scope. They live in the prelude (with the `derive`
feature, which is on by default):

```rust
use treat::prelude::*;
```

If you disabled default features, re-enable `derive`.

## "the trait bound `T: ApiErrorCode` is not satisfied"

`ApiErrorCode` is a marker trait with a blanket impl: every type that is
`Debug + Clone + Display + Send + Sync` implements it, and nothing else can (an
`impl ApiErrorCode for MyCode {}` of your own conflicts with the blanket one).
`ApiError<C>` additionally needs `C: 'static`. So the compiler is telling you
which of those your code type lacks — usually `Display`. For a custom code
enum, `#[derive(ApiErrorCode)]` generates `Display` (and the `From<MyCode>` for
`ApiError<MyCode>`); derive `Clone` and `Debug` next to it.

`with_code_status()` needs one more trait, `ApiErrorStatus`. The derive emits
it as soon as one variant carries `#[status(..)]`.

## "the trait bound `MyData: ToSchema` is not satisfied"

You enabled the `openapi` feature, which makes `ResponseData = Serialize +
utoipa::ToSchema`. Add `#[derive(utoipa::ToSchema)]` to the type you use as
`data` / `meta`, or drop the `openapi` feature.

## "`error` is ambiguous" / clashes with my own `error`

The free function is intentionally **not** in the prelude. Call it fully
qualified: `treat::error("code")`.

## My `ApiError` response only shows one error, but there's a cause chain

That's the default — only the top-level error is serialized. Opt in to the full
chain with `.with_verbose()` on the error, or the `verbose-error` feature
globally. See [errors.md](errors.md#verbose-mode-what-reaches-the-client).

## `{:?}` and `{:#?}` print different things

They are two views of the same error:

- `{:?}` is the one to log. It prints the error, its cause chain under
  `Caused by:`, and every location under `Location:`, outermost first: the
  last [hop](errors.md#tracking-better-debug-trails), back to where the error
  was raised, then down the source chain. With `spantrace` / `backtrace` the
  captured trace follows.
- `{:#?}` dumps the struct field by field, for inspecting one error in a test
  or a debugger.

For structured output rather than text, use `err.collect_messages()` or
`err.to_error_message()`.

## "`#![feature]` may not be used on the stable release channel"

`nightly-provide` and `tracked` need a nightly toolchain; every other feature
builds on stable. `tracked` fails inside `erris`, which it turns on in erris's
`tracked` mode.

That mode is also not additive: it makes `erris::Result` a `TrackedResult`
across the whole build, your own code included. A dependency that enables
`treat/tracked` (or `erris/tracked`) changes `erris::Result` for you too, so
enable it in the application, deliberately, and not in a library.

## Publishing the workspace

`just publish` runs `cargo publish --workspace`, which orders the seven
publishable crates (`treat`, `treat-core`, `treat-derive`, `treat-actix`,
`treat-tower`, `treat-rpc`, `treat-axum-logging`) by their dependencies and
waits for the registry index between uploads. The two `treat-hygiene*` members
are `publish = false` and are skipped. You need the registry token in the
environment.
