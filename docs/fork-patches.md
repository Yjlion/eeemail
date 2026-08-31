# Fork patch ledger

`core/` is a fork of [`chatmail/core`](https://github.com/chatmail/core)
(MPL-2.0). Upstream is tracked as a git remote and merged periodically.

**Fork discipline.** Whether this fork stays maintainable is decided entirely by
how disciplined we are here:

1. Put new code in `core/src/email/`. Nothing that can live there should live
   anywhere else.
2. Patch upstream files as narrowly as possible — ideally a single call site or
   hook point per file.
3. Record every upstream-file patch in the table below, with the reason. A merge
   conflict is usually resolved by someone who wasn't there when the patch was
   written; this table is what they will read.
4. Prefer disabling unwanted upstream features behind a cargo feature over
   deleting them. Group machinery, `webxdc.rs` and `peer_channels.rs` are woven
   through `receive_imf/` and `chat/`; early deletion means fighting the
   compiler instead of building.

## Upstream

- Remote: `upstream` -> `https://github.com/chatmail/core`
- Vendored with `git subtree` at prefix `core/`, full upstream history retained.
- Forked at: **`v2.59.0`** (commit `e322fdf157d8573db6e57aeefb7d3cdb1b272b19`, 2026-08-14)
- Last merged: _(none yet -- fork is at the initial import)_

The fork point is also recorded machine-readably in `docs/fork-base`, which
`scripts/check-fork-patches.sh` reads. Update both together.

We fork from **tagged releases**, not `main`. Releases are reproducible and have
been through upstream's own release testing; `main` has not.

### Merging from upstream

```sh
git fetch upstream --tags
git tag -l 'v*' | sort -V | tail -5     # pick the next release to move to
git subtree pull --prefix=core <tag>
```

Then, in the same pull request:

1. Resolve conflicts. The table below tells you what each of our patches to an
   upstream file was for; preserve the intent, not necessarily the diff.
2. Update `docs/fork-base` to the new upstream commit and "Last merged" above.
3. Re-run the full test suite (`cd core && cargo nextest run --workspace`;
   see [`testing.md`](testing.md) for why not `cargo test`).
4. Re-run the interop tests -- Autocrypt and SecureJoin against a real Delta
   Chat client. Upstream changes crypto and protocol code routinely, and a green
   unit-test run does not prove we still interoperate.

Never merge upstream and change our own behavior in the same commit. If a merge
needs adaptation, land the merge first and the adaptation second, so a bisect
can tell them apart.

## Patches to upstream files

| File | Change | Why | ADR |
|---|---|---|---|
| `core/Cargo.toml` | Added `relay-provisioning`, `peer-channels`, `webxdc` features; all three included in `default`. | Declares the gates for out-of-scope upstream features. In `default` for now so the fork builds and tests identically to upstream; each is dropped from `default` as it is actually gated. Inventory: [`out-of-scope.md`](out-of-scope.md). | [0001](adr/0001-fork-chatmail-core.md), [0007](adr/0007-server-template.md) |

Conflict guidance for `core/Cargo.toml`: upstream edits `[features]` rarely, but
when it does, keep both sides -- our three feature keys are additive and our only
change to `default` is appending to the existing list.

## New files (not upstream, no merge risk)

| File | Purpose | ADR |
|---|---|---|
| `core/src/email/rawmime.rs` | Raw MIME blob store + retention/expiry | [0004](adr/0004-local-store-and-raw-mime.md) |
| `core/src/email/recipients.rs` | To/CC/BCC sets decoupled from chat membership | [0001](adr/0001-fork-chatmail-core.md) |
| `core/src/email/threading.rs` | JWZ threading over References/In-Reply-To | [0001](adr/0001-fork-chatmail-core.md) |
| `core/src/email/labels.rs` | Labels/tags + archive | [0005](adr/0005-labels-not-folders.md) |
| `core/src/email/policy.rs` | Encryption strictness, retention, MDN/ephemeral defaults | [0006](adr/0006-encryption-policy.md) |
