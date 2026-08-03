set export
set dotenv-load

PROFILE := env_var_or_default("PROFILE", "release")

default:
    @just --list

PWD := invocation_directory()

# Install the cargo subcommands the other recipes call.
install:
    # `set-version` (used by `publish`) ships in cargo-edit; `fmt` and `clippy`
    # come from rustup and are not installed here.
    cargo install --locked cargo-deny cargo-edit cargo-machete cargo-nextest cargo-sort cargo-workspace-lints

lint:
    cargo deny --log-level error check advisories bans sources
    cargo fmt --all --check -- --unstable-features --error-on-unformatted
    cargo check
    cargo workspace-lints
    cargo clippy
    cargo sort -c -w
    cargo machete

fix:
    cargo clippy --fix --allow-dirty --allow-staged --all-features --all-targets
    cargo fmt --all -- --unstable-features --error-on-unformatted
    cargo sort -w
    cargo machete --fix

# `cargo publish --workspace` publishes every member, ordering them by their
# inter-dependencies and waiting for the registry index between uploads.
publish:
    cargo set-version ${CI_COMMIT_TAG#[vV]}
    cargo publish --workspace --allow-dirty --locked --no-verify $@

test *args='':
    cargo nextest run --run-ignored default {{ if PROFILE == "release" { "--release" } else { "" } }} $args

test-integration *args='':
    cargo nextest run --run-ignored ignored-only {{ if PROFILE == "release" { "--release" } else { "" } }} $args

test-all *args='':
    cargo nextest run --run-ignored all {{ if PROFILE == "release" { "--release" } else { "" } }} $args
