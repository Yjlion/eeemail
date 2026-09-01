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
| `core/src/config.rs` | Doc comment on `ForceEncryption` explaining that eeemail turns it off at setup rather than changing its default. Added `Config::RawMimeRetentionDays` (default `30`), `Config::EncryptionMode` (default `1`), `Config::ServerRetentionDays` (default `0`), `Config::MdnPolicy` (default `2`) and `Config::EphemeralDefaultSeconds` (default `0`). | Raw-MIME retention; encryption strictness, which only distinguishes opportunistic from lenient since `ForceEncryption` stays authoritative for strict-vs-not; how long a downloaded message is left on the server. | [0004](adr/0004-local-store-and-raw-mime.md), [0006](adr/0006-encryption-policy.md), [0010](adr/0010-server-retention.md) |
| `core/src/lib.rs` | `pub mod email;`. | Registers our layer. | [0001](adr/0001-fork-chatmail-core.md) |
| `core/src/receive_imf.rs` | One best-effort block at the single success exit of `receive_imf_inner`, calling `email::rawmime::store`, `email::recipients::store`, `email::threading::assign_stored`, `email::labels::drain_pending` and `email::policy::apply_server_retention`. | Retains incoming originals, records the recipient set, places the message in a conversation, applies labels synced ahead of the message, and lets the server forget it once it is stored locally. Deliberately not on the `trash()` path: an ignored message has no content worth keeping. | [0004](adr/0004-local-store-and-raw-mime.md), [0008](adr/0008-email-message-model.md) |
| `core/src/chat.rs` | In `create_send_msg_jobs`: a `mimefactory.to_header()` capture beside the existing `mimefactory.recipients()` call; `email::policy::prepare_send` and `email::receipts::apply_default_timer` calls immediately before `needs_encryption` is computed; and a best-effort block before the SMTP-queue transaction mirroring the receive hook, plus `email::policy::record_undelivered`. | Gives sent messages the same raw MIME, recipient set and thread as received ones, applies the effective encryption mode, and records who the message was addressed to but not sent to. The `to_header` capture is needed because `mimefactory` is consumed by rendering. **`prepare_send` is the one hook here that is not best-effort**: it decides whether the message is encrypted, so a failure must stop the send rather than quietly downgrade it. | [0004](adr/0004-local-store-and-raw-mime.md), [0008](adr/0008-email-message-model.md) |
| `core/src/context.rs` | Added `raw_mime_retention_days`, `encryption_mode`, `server_retention_days`, `mdn_policy` and `ephemeral_default_seconds` to the `get_info` map. | Upstream's `test_get_info_completeness` requires every `Config` key to appear in `get_info` or be explicitly skipped. Included rather than skipped because it is exactly what you check when "view source" is unavailable on an older message. | [0004](adr/0004-local-store-and-raw-mime.md) |
| `core/src/sql.rs` | `email::rawmime::expire`, `email::recipients::prune`, `email::threading::prune`, `email::labels::prune`, `email::labels::prune`, `email::policy::expire_on_server`, `email::policy::prune` and `email::receipts::prune` calls in `housekeeping`, plus a `SELECT blobname FROM raw_mime` block in `remove_unused_files`. | Expiry must run **before** reference collection so freed blobs are reclaimed in the same pass; the SELECT stops housekeeping deleting blobs that are still retained. Mirrors the existing `http_cache` and `msgs_mdns` blocks. | [0004](adr/0004-local-store-and-raw-mime.md), [0008](adr/0008-email-message-model.md) |
| `core/src/sql/migrations.rs` | Migration 165: `CREATE TABLE msg_recipients`, `threads`, `msg_threads`, `thread_refs`. | Per-message recipient sets and thread grouping. | [0008](adr/0008-email-message-model.md) |
| `core/src/mimeparser.rs` | Added a `header_recipients` field to `MimeMessage`, one extra `&mut` parameter to `merge_headers` and one line setting it, and widened `get_all_addresses_from_header` to `pub(crate)`. | `To` and `Cc` must be kept apart; upstream's `recipients` concatenates them. Taken from `merge_headers` because that is where RFC 9788 protected headers have already been resolved -- the outer headers of an encrypted message may be absent or misleading. | [0008](adr/0008-email-message-model.md) |
| `core/src/mimefactory.rs` | Added a `to_header()` accessor beside the existing `recipients()`. | Records what a sent message was addressed to. Additive; no behaviour change. | [0008](adr/0008-email-message-model.md) |
| `core/src/sql/migrations.rs` | Migration 166: `CREATE TABLE labels`, `msg_labels`, `pending_msg_labels`, and the reserved `Archive` row. | Labels, tags and archive. | [0009](adr/0009-labels-and-search.md) |
| `core/src/sql/migrations.rs` | Migration 167: `CREATE TABLE contact_policy`, `server_retention`, `msg_undelivered`. | Per-contact encryption overrides, deferred server deletion, and recipients a message never reached. | [0006](adr/0006-encryption-policy.md), [0010](adr/0010-server-retention.md) |
| `core/src/sync.rs` | Added one `SyncData::EmailLabel` variant and one match arm in `execute_sync_items`. | Replays label changes on the user's other devices. One variant wrapping an enum defined in our own module keeps the merge surface to a single arm; older builds fall into upstream's existing `SyncDataOrUnknown::Unknown` path. | [0009](adr/0009-labels-and-search.md) |
| `core/src/sql/migrations.rs` | Migration 168: two `ALTER TABLE contact_policy ADD COLUMN` for `mdn_enabled` and `ephemeral_secs`. | Per-contact read-receipt and ephemeral overrides. | [0011](adr/0011-receipts-and-ephemeral.md) |
| `core/src/message.rs` | Replaced the `context.should_send_mdns()` call in the MDN decision with `email::receipts::should_send_mdn(context, curr_from_id)`. | Read receipts are a disclosure, so who gets one depends on the correspondent; upstream's version takes no contact. One call site, and the only one. | [0011](adr/0011-receipts-and-ephemeral.md) |
| `core/Cargo.toml` | Added `"../cli"` to `[workspace] members`. | Puts `eeemail-cli` in this workspace so it shares the build and lockfile instead of compiling `deltachat` a second time, while living outside `core/` so the fork stays the fork. | [0012](adr/0012-rpc-and-cli.md) |
| `core/Cargo.lock` | Regenerated. | Derived, not hand-written: the consequence of the `Cargo.toml` change above. On conflict, take either side and re-run `cargo check`. | [0012](adr/0012-rpc-and-cli.md) |
| `core/deltachat-jsonrpc/src/api.rs` | Two imports, a `timer_from_secs` helper at the end of the file, and one contiguous block of ~30 thin methods at the end of `impl CommandApi`. | `yerpc`'s `#[rpc]` attribute can only be applied to one `impl` block, so our methods cannot live in a separate file. Every one is a direct call into `deltachat::email::*` and the types are in `api/types/email.rs`, so on conflict re-place the block rather than replaying the diff. | [0012](adr/0012-rpc-and-cli.md) |
| `core/deltachat-jsonrpc/src/api/types/mod.rs` | `pub mod email;`. | Registers our types. | [0012](adr/0012-rpc-and-cli.md) |

Conflict guidance for the raw-MIME hooks: all three are single call sites whose
*intent* is what matters -- retain originals on receive and send, expire them in
housekeeping. If upstream restructures `receive_imf_inner`'s exits or
`create_send_msg_jobs`, re-place the call rather than replaying the diff. The
ordering constraint in `sql.rs` (expire before `remove_unused_files`) is load
bearing and covered by
`email::rawmime::rawmime_tests::test_housekeeping_keeps_retained_blob_but_reclaims_expired`.

Conflict guidance for the email-model hooks: `merge_headers` is the one patch
here that upstream is likely to touch, because it gains parameters as upstream
adds protected headers. The intent is narrow -- whenever upstream replaces
`recipients`, replace our `to`/`cc` pair on the same condition. If the signature
becomes unwieldy upstream may collapse it into a struct; fold
`header_recipients` into that struct rather than fighting it. The rest of the
hooks are single call sites; re-place them by intent as with the raw-MIME ones.

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
| `core/src/email/recipients.rs` | To/CC/BCC sets decoupled from chat membership | [0008](adr/0008-email-message-model.md) |
| `core/src/email/threading.rs` | JWZ threading over References/In-Reply-To | [0008](adr/0008-email-message-model.md) |
| `core/src/email/labels.rs` | Labels/tags + archive, and their device sync | [0009](adr/0009-labels-and-search.md) |
| `core/src/email/search.rs` | Search over body, subject, recipients and labels | [0009](adr/0009-labels-and-search.md) |
| `core/src/email/policy.rs` | Encryption strictness, per-contact overrides, undelivered recipients, server retention | [0006](adr/0006-encryption-policy.md), [0010](adr/0010-server-retention.md) |
| `core/src/email/receipts.rs` | Read-receipt policy and ephemeral defaults, with per-contact overrides | [0011](adr/0011-receipts-and-ephemeral.md) |
| `core/deltachat-jsonrpc/src/api/types/email.rs` | JSON-RPC types for the email layer | [0012](adr/0012-rpc-and-cli.md) |
| `cli/` | Headless driver for development and integration tests | [0012](adr/0012-rpc-and-cli.md) |
