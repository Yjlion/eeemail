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

If you use the **`gh` CLI**, run this once in a fresh clone:

```sh
gh repo set-default Yjlion/eeemail
```

`gh` prefers a remote named `upstream`, and ours points at `chatmail/core` -- so
until you do, every `gh` command reads and writes *Delta Chat's* repository, and
`gh pr create` fails with `No commits between main and <branch>` rather than
saying so. The setting lives in `.git/config`, so it is per-clone and cannot be
committed. Leave the `upstream` remote alone; it is what upstream merges are
fetched from.

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

**A portable copy**, which is what the release `.zip` is. `tauri build` writes
the app beside the two tools; the empty `eeemail-portable` marker next to the
executable is the whole of what makes it portable, so this is enough to try the
rule out:

```sh
cd desktop && npm run tauri build -- --debug --bundles deb
mkdir -p /tmp/portable-check
cp ../core/target/debug/eeemail /tmp/portable-check/
touch /tmp/portable-check/eeemail-portable
/tmp/portable-check/eeemail
# the profile appears in /tmp/portable-check/data/, and nothing is written to
# ~/.local/share/eeemail
```

See [ADR 0024](adr/0024-portable-archives.md) and
[`PORTABLE.md`](PORTABLE.md).

**Run the shell after touching it.** Nothing automated exercises the real Tauri
IPC path: `screenshots.sh` photographs the browser demo build above, which never
calls `invoke`, and `e2e-pass.py` drives `deltachat-rpc-server`, which the app
does not use. v0.3.0 shipped an app with no `capabilities/` directory, so
Tauri's ACL compiled to `{}`, `listen()` was refused, and every check in this
repository was green. `desktop/src-tauri/capabilities/` is source and must be
committed; `desktop/src-tauri/gen/` is generated and is not. Commands registered
in `generate_handler!` need no ACL entry, but anything from a Tauri plugin --
the core `event` plugin included -- does.

`test_the_frontend_is_allowed_to_hear_the_engine` guards it by asking the
compiled `RuntimeAuthority` the same question the running app asks. If it fails
while the capability file looks correct, it is the build cache: `tauri-build`
re-reads `capabilities/` on cargo's mtime fingerprint, so restoring a file with
its old timestamp leaves the previous ACL compiled into that feature set's
`OUT_DIR`. `touch desktop/src-tauri/capabilities/*.json
desktop/src-tauri/build.rs` and rebuild. CI always builds cold and never sees
it.

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
