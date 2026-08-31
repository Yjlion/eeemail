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

| `core/src/lib.rs` | `#[cfg(feature = "relay-provisioning")]` on `mod automatic_relay_management;`. | Gates chatmail relay provisioning off. | [0007](adr/0007-server-template.md) |
| `core/src/qr.rs` | Split the `DCACCOUNT:` implementation into feature-gated real versions of `decode_account` / `login_param_from_account_qr` plus `#[cfg(not(...))]` stubs that `bail!`. Gated the now relay-only imports, `HTTPS_SCHEME`, and the `CreateAccount*Response` structs. | Same. Stubs rather than gating the `Qr::Account` variant, so no enum definition or `match` arm needs a `cfg` -- a much smaller merge surface. | [0007](adr/0007-server-template.md) |
| `core/src/net/http.rs` | `#[cfg(feature = "relay-provisioning")]` on `post_empty`. | Only caller is the gated `DCACCOUNT:` path; unused otherwise and CI runs `-Dwarnings`. | [0007](adr/0007-server-template.md) |
| `core/src/imap/idle.rs` | `#[cfg(feature = "relay-provisioning")]` on the `maybe_add_additional_relays` spawn before IDLE. | Same. | [0007](adr/0007-server-template.md) |
| `core/src/qr/qr_tests.rs` | `#[cfg(feature = "relay-provisioning")]` on `test_decode_account`. | The test asserts `DCACCOUNT:` parses; meaningless once gated off. | [0007](adr/0007-server-template.md) |

| `core/src/sql/migrations.rs` | Migration 164: `CREATE TABLE raw_mime`. | Raw MIME retention storage. | [0004](adr/0004-local-store-and-raw-mime.md) |
| `core/src/config.rs` | Added `Config::RawMimeRetentionDays` (default `30`). | Retention period: 0 = off, negative = forever. | [0004](adr/0004-local-store-and-raw-mime.md) |
| `core/src/lib.rs` | `pub mod email;`. | Registers our layer. | [0001](adr/0001-fork-chatmail-core.md) |
| `core/src/receive_imf.rs` | One best-effort `email::rawmime::store` call at the single success exit of `receive_imf_inner`. | Retains incoming originals. Deliberately not on the `trash()` path: an ignored message has no content worth keeping. | [0004](adr/0004-local-store-and-raw-mime.md) |
| `core/src/chat.rs` | One best-effort `email::rawmime::store` call in `create_send_msg_jobs`, before the SMTP-queue transaction. | Retains outgoing originals so a sent message can be viewed as source like a received one. | [0004](adr/0004-local-store-and-raw-mime.md) |
| `core/src/context.rs` | Added `raw_mime_retention_days` to the `get_info` map. | Upstream's `test_get_info_completeness` requires every `Config` key to appear in `get_info` or be explicitly skipped. Included rather than skipped because it is exactly what you check when "view source" is unavailable on an older message. | [0004](adr/0004-local-store-and-raw-mime.md) |
| `core/src/sql.rs` | `email::rawmime::expire` call in `housekeeping`, plus a `SELECT blobname FROM raw_mime` block in `remove_unused_files`. | Expiry must run **before** reference collection so freed blobs are reclaimed in the same pass; the SELECT stops housekeeping deleting blobs that are still retained. Mirrors the existing `http_cache` block exactly. | [0004](adr/0004-local-store-and-raw-mime.md) |

Conflict guidance for the raw-MIME hooks: all three are single call sites whose
*intent* is what matters -- retain originals on receive and send, expire them in
housekeeping. If upstream restructures `receive_imf_inner`'s exits or
`create_send_msg_jobs`, re-place the call rather than replaying the diff. The
ordering constraint in `sql.rs` (expire before `remove_unused_files`) is load
bearing and covered by
`email::rawmime::rawmime_tests::test_housekeeping_keeps_retained_blob_but_reclaims_expired`.

Conflict guidance for `core/Cargo.toml`: upstream edits `[features]` rarely, but
when it does, keep both sides -- our three feature keys are additive and our only
change to `default` is appending to the existing list.

Conflict guidance for the `relay-provisioning` gates: upstream renamed
`automatic_relay_management` to `autorelay` after our fork point, so the next
merge will move `core/src/lib.rs` and `core/src/imap/idle.rs`. The intent is
simply that **no chatmail relay provisioning code is reachable by default**. If
upstream restructures the `DCACCOUNT:` path, re-derive the gates from that
intent rather than trying to replay this diff.

## New files (not upstream, no merge risk)

| File | Purpose | ADR |
|---|---|---|
| `core/src/email/rawmime.rs` | Raw MIME blob store + retention/expiry | [0004](adr/0004-local-store-and-raw-mime.md) |
| `core/src/email/recipients.rs` | To/CC/BCC sets decoupled from chat membership | [0001](adr/0001-fork-chatmail-core.md) |
| `core/src/email/threading.rs` | JWZ threading over References/In-Reply-To | [0001](adr/0001-fork-chatmail-core.md) |
| `core/src/email/labels.rs` | Labels/tags + archive | [0005](adr/0005-labels-not-folders.md) |
| `core/src/email/policy.rs` | Encryption strictness, retention, MDN/ephemeral defaults | [0006](adr/0006-encryption-policy.md) |
