# Development setup

## Requirements

- **Rust** — MSRV 1.89, CI lints on 1.97.1. `core/` is edition 2024.
- **Docker** — for the `server/` integration test target (Phase 0.5, not yet built).

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
```

No system OpenSSL or SQLite is needed: `core`'s default `vendored` feature
builds SQLCipher and OpenSSL from source. The first build is slow (10-20
minutes) and pulls a large dependency tree.

## Layout

`core/` is a `git subtree` fork of [`chatmail/core`](https://github.com/chatmail/core)
and is **its own cargo workspace**. Build and test from inside it:

```sh
cd core
cargo check --workspace --all-targets
cargo nextest run --workspace
cargo test --workspace --locked --doc
cargo clippy --workspace --all-targets --all-features
cargo fmt --all -- --check
```

Use `cargo nextest`, not `cargo test` — upstream's clock mock is process-global,
so a single-process run fails on a green tree. Install with
`cargo install cargo-nextest --locked`. See [`testing.md`](testing.md).

## Before you touch `core/`

Read [ADR 0001](adr/0001-fork-chatmail-core.md). The short version:

- New code goes in `core/src/email/`. It is exempt from the checks below.
- Patching an upstream file means recording it in
  [`fork-patches.md`](fork-patches.md). CI enforces this:

  ```sh
  ./scripts/check-fork-patches.sh
  ```

- Prefer a cargo feature gate over deleting an upstream feature. See
  [`out-of-scope.md`](out-of-scope.md).

## Merging from upstream

See [`fork-patches.md`](fork-patches.md#merging-from-upstream). Note that a green
test run does **not** prove we still interoperate — upstream changes crypto and
protocol code routinely, so `scripts/interop-pass.py`, which runs eeemail
against upstream's own released binary, is part of every merge. Move the pin in
[`interop-upstream`](interop-upstream) with [`fork-base`](fork-base).
