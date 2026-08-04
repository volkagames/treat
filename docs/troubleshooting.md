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

`ApiError<C>` requires `C: Debug + Clone + Display + Send + Sync`. For a custom
code enum, either `#[derive(ApiErrorCode)]` (which generates `Display`) or
implement `Display` yourself, and make sure `Clone + Debug` are derived.

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

## `Debug` shows a location/backtrace pointing into the library

The non-alternate `{:?}` currently renders the chain via a fresh report, so its
`Location:`/`Backtrace:` section can point at the `Debug` call site rather than
where the error was created. Use `{:#?}` (alternate) for the exact captured
`location`, or `err.collect_messages()` / `err.to_error_message()` for stable,
structured output. This is a known issue.

## `cargo +stable build` fails with "-Z may only be used on nightly"

The **repo's** `.cargo/config.toml` sets a nightly-only `-Zshare-generics`
rustflag for local builds. It does not affect downstream crates. To build this
repo on stable, clear the flag for the invocation:

```sh
RUSTFLAGS= cargo +stable test
```

## I need a nightly feature (`Error::provide`)

Enable `nightly-provide` and use a nightly toolchain. Everything else works on
stable.

## Publishing the workspace

`just publish` runs `cargo publish --workspace`, which orders the five crates by
their dependencies and waits for the registry index between uploads. You need the
registry token in the environment.
