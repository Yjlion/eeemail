# CLAUDE.md

Guidance for working in this repository. Read
[`docs/handoff.md`](docs/handoff.md) first — it is the living state document and
says what is true right now, including what is broken.

## What this is

eeemail is a classic email client built on a **fork of `chatmail/core`** (Delta
Chat's engine), using its encryption for ordinary mail. IMAP and SMTP are
transport only: mail is downloaded, decrypted, stored locally and removed from
the server, so **the local database is the mailbox**. Unaudited, prerelease.

## Layout, and why the target directory is where it is

| Path | What it is |
|---|---|
| `core/` | The fork of `chatmail/core`. **Also the cargo workspace root.** |
| `cli/` | `eeemail-cli` — inspects a mailbox. One-shot, no daemon, so it cannot send or receive. |
| `desktop/` | Vite/TypeScript frontend in `src/`, Tauri shell in `src-tauri/`. |
| `server/compose/` | A Postfix/Dovecot server the live passes run against. |
| `scripts/` | The three live passes, the screenshot regenerator, the fork-patch check. |
| `docs/adr/` | One decision per file. |

`cli/` and `desktop/src-tauri/` live outside `core/` so the fork stays the fork,
but are members of its workspace (`core/Cargo.toml`, `[workspace] members`) so
they share the build and the lockfile. Cargo only searches *parent* directories,
which is why each spells out `workspace = "../../core"`.

**Consequence: everything builds into `core/target/`, and cargo runs from inside
`core/`.**

## Testing

**Use `cargo nextest`, never `cargo test`.** `deltachat-time` mocks the clock
through a process-global whose shift accumulates, so a single-process run fails
on a green tree. nextest gives each test its own process. See
[`docs/testing.md`](docs/testing.md).

**Run both configurations.** They are not the same suite:

```sh
cd core
cargo nextest run --workspace                 # ~1376 tests
cargo nextest run --workspace --all-features  # ~1386 tests
```

`--all-features` carries ten tests the default build does not. A green default
run is not a green CI.

Full gate, all of which CI runs:

```sh
cd core && cargo nextest run --workspace && cargo nextest run --workspace --all-features
cargo test --workspace --locked --doc
cargo clippy --workspace --all-targets -- -D warnings   # and again with --all-features
cargo fmt --all -- --check
cd .. && ./scripts/check-fork-patches.sh
cd desktop && npm run check && npm run build
./scripts/screenshots.sh    # 11 images, must be byte-stable across runs
```

The live passes need Docker and a built `deltachat-rpc-server`; see
`docs/handoff.md` and [`docs/DESIGN.md`](docs/DESIGN.md#verification).

`test_cache_is_cleared_when_io_is_started` is flaky upstream and is not ours.

## Fork discipline

New code goes in **`core/src/email/`**. Every touch of an upstream file is
recorded in [`docs/fork-patches.md`](docs/fork-patches.md) and enforced by
`./scripts/check-fork-patches.sh`, which CI runs. A patch that is not in the
ledger fails the build; the ledger is what makes the next upstream merge
possible. Read [`docs/development.md`](docs/development.md) before touching
`core/`.

### `gh` points at Delta Chat unless you tell it not to

This clone has two remotes: `origin` is `Yjlion/eeemail`, `upstream` is
`chatmail/core`. **`gh` prefers a remote named `upstream`**, so in a fresh clone
every `gh` command resolves to Delta Chat's repository rather than ours.
`gh repo view` reports `chatmail/core`, and `gh pr create` tries to open a pull
request *against Delta Chat* -- which fails with `No commits between main and
<branch>`, a message that says nothing about the actual problem.

One command, once per clone:

```sh
gh repo set-default Yjlion/eeemail
```

It writes `remote.origin.gh-resolved = base` into `.git/config`, which is
per-clone and **not** committed -- so this is not something the repository can
fix for you, and a new checkout needs it again. Until you run it, pass
`--repo Yjlion/eeemail` to every `gh` command.

Do not fix this by renaming or deleting the `upstream` remote. It is what the
next merge from `chatmail/core` is fetched from, and the fork-patch ledger
exists to make that merge possible.

## Decisions and documents

- **ADRs are immutable.** To change one, add a new ADR that supersedes it, or a
  dated amendment block inside it. Never edit the original text. Add a row to
  `docs/adr/README.md` either way.
- **`docs/handoff.md` is updated with what each release taught**, not with what
  was planned. It is honest about gaps on purpose; keep it that way.
- Commit subjects are imperative, sentence case, no prefixes or scopes ("Stop
  writing the subject into the body of classic mail"), with a wrapped
  explanatory body saying *why*.

## The desktop app is the least-tested thing here

Nothing automated exercises the real app. `scripts/screenshots.sh` renders a
**browser-only demo build** (`VITE_EEEMAIL_DEMO=1`, answered from
`desktop/src/fixtures.ts`) with no Tauri IPC at all, and `scripts/e2e-pass.py`
drives `deltachat-rpc-server`, which **the app does not use** — the shell embeds
the engine in-process. So the whole IPC path can be broken with every check
green. It has been, twice:

- `accounts_dir()` read `XDG_DATA_HOME`/`HOME` and errored on Windows, for eight
  releases of green Windows builds.
- v0.3.0 shipped with no `desktop/src-tauri/capabilities/`, so Tauri's ACL
  compiled to `{}`, `listen()` was refused and no engine event ever reached the
  frontend.

If you change the shell, the frontend's IPC, or `tauri.conf.json`, **run it**:

```sh
cd desktop && EEEMAIL_ACCOUNTS_DIR=/tmp/eeemail npm run tauri dev
```

`desktop/src-tauri/gen/` is generated and gitignored;
`desktop/src-tauri/capabilities/` is source and must be committed. App commands
registered in `generate_handler!` need no ACL entry; anything from a Tauri
plugin, core plugins included, does.
`test_the_frontend_is_allowed_to_hear_the_engine` asks the compiled
`RuntimeAuthority` the same question the running app does, and is the guard.

**If that test fails locally and the capability file looks right, suspect the
build cache.** `tauri-build` re-reads `capabilities/` on cargo's mtime
fingerprint, so restoring a file with its old timestamp -- `git stash pop`, a
branch switch, moving the directory away and back -- leaves the previous ACL
compiled into that feature set's `OUT_DIR`. `touch
desktop/src-tauri/capabilities/*.json desktop/src-tauri/build.rs` and rebuild.
CI never sees this; it always builds cold.

## Things that will bite you

The five traps in [`docs/handoff.md`](docs/handoff.md) — read them there rather
than trusting a summary. In short: migration 171 renames data three ways and
each fails silently; `UnverifiedTrashDays` reads `0` as *never sweep* while
`TrashPurgeDays` reads it as *destroy now*, deliberately; the npm and Rust Tauri
versions must move together and are pinned exact; the unverified deadline is
computed at sweep time and must not become stored state; one screenshot reads
the wall clock and so is not actually deterministic.

Two more from `docs/handoff.md`:

- **`email::ephemeral::divert` must stay above `select_expired_messages`.** Its
  position *is* the patch. Moving it compiles, passes most tests, and silently
  restores destructive expiry.
- **eeemail's defaults are applied at setup, not compiled in.**
  `email::policy::apply_defaults` never touches a configured account, so every
  caller must run it *before* configuring. The GUI had this backwards for two
  phases.

## Pointers

[`docs/DESIGN.md`](docs/DESIGN.md) · [`docs/development.md`](docs/development.md)
· [`docs/testing.md`](docs/testing.md) · [`docs/INSTALL.md`](docs/INSTALL.md) ·
[`docs/PORTABLE.md`](docs/PORTABLE.md) · [`docs/adr/`](docs/adr/README.md) ·
[`docs/out-of-scope.md`](docs/out-of-scope.md)
