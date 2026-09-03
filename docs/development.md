# Development setup

## Requirements

- **Rust** — MSRV 1.89, CI lints on 1.97.1. `core/` is edition 2024.
- **Node 22+** — for the desktop frontend.
- **Docker** — for `server/compose`, the test mail server the live passes run
  against.
- **Linux desktop libraries**, for the Tauri shell:
  `libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev
  libgtk-3-dev librsvg2-dev patchelf`.

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

## Running it

**The desktop app**, against a real account:

```sh
cd desktop
npm ci
npm run tauri dev
```

`EEEMAIL_ACCOUNTS_DIR=/tmp/eeemail npm run tauri dev` points it at a scratch
mailbox instead of your own.

The first launch shows the disclosure dialog once and then writes
`<data dir>/first-run-acknowledged` — see the table in
[`INSTALL.md`](INSTALL.md#where-your-mail-lives) for where that is. Delete it to
see the dialog again. Note that `EEEMAIL_ACCOUNTS_DIR` moves the accounts and
**not** the marker, which is deliberate: the disclosure is about the software,
and the person who read it does not un-read it by pointing the app at a
different mailbox.

**The UI alone**, with no engine and no mailbox, answering from
`desktop/src/fixtures.ts`:

```sh
cd desktop && npm run build:demo && npm run preview:demo
```

That is what `scripts/screenshots.sh` photographs, which is why the images are
reproducible.

**Installers**, the artefacts a release publishes:

```sh
cd desktop && npm run tauri build -- --bundles deb,appimage
# core/target/release/bundle/{deb,appimage}/
```

Add `--debug` to bundle the dev-profile binary, which is how to check the
packaging path without waiting for an LTO release build.

`@tauri-apps/api` and `@tauri-apps/cli` are pinned to **exact** versions in
`package.json`, not caret ranges. `tauri build` refuses to run when the
`tauri` crate and `@tauri-apps/api` differ in major/minor, and a caret range is
what let them drift apart unnoticed while the release still used `cargo build`.
Moving either means moving both -- and the Rust side is capped by
`rust-version = 1.89`, which CI gates on.

**The CLI.** One-shot: it opens the account, does one thing, prints JSON and
exits. It never starts the IO loop, so it can neither send nor receive.

```sh
cd core && cargo run -p eeemail-cli -- <path-to>/dc.db info
```

**The JSON-RPC server**, which is what the live passes in `scripts/` drive:

```sh
cd core && cargo build -p deltachat-rpc-server
DC_ACCOUNTS_PATH=/tmp/eeemail ./target/debug/deltachat-rpc-server
```

See [`INSTALL.md`](INSTALL.md) for what an end user gets, which is worth
reading before changing any of the above.

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
