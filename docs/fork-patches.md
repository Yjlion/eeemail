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
2. Update `docs/fork-base` to the new upstream commit and "Last merged" above,
   and `docs/interop-upstream` to the tag you merged to. The two move together:
   the interop pass is only unambiguous when it runs against the release we are
   actually forked from. `scripts/interop-pass.py --fetch-only` prints the hash
   it observed when the recorded one no longer matches, so re-recording it is
   copy-paste.
3. Re-run the full test suite (`cd core && cargo nextest run --workspace`;
   see [`testing.md`](testing.md) for why not `cargo test`).
4. Re-run `scripts/interop-pass.py` -- Autocrypt and SecureJoin against
   upstream's own released binary. Upstream changes crypto and protocol code
   routinely, and a green unit-test run does not prove we still interoperate.
   Read this ledger's note on ADR 0021 first: it is the one place we diverge
   from something upstream changed deliberately, so it is the first thing a
   merge will break.

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
| `core/Cargo.toml` | Added `"../cli"` and `"../desktop/src-tauri"` to `[workspace] members`. | Puts our own crates in this workspace so they share the build and lockfile instead of compiling `deltachat` two more times, while living outside `core/` so the fork stays the fork. | [0012](adr/0012-rpc-and-cli.md), [0013](adr/0013-desktop-ui.md) |
| `core/Cargo.lock` | Regenerated. | Derived, not hand-written: the consequence of the `Cargo.toml` change above. On conflict, take either side and re-run `cargo check`. | [0012](adr/0012-rpc-and-cli.md) |
| `core/deltachat-jsonrpc/src/api.rs` | Two imports, a `timer_from_secs` helper at the end of the file, and one contiguous block of ~30 thin methods at the end of `impl CommandApi`. | `yerpc`'s `#[rpc]` attribute can only be applied to one `impl` block, so our methods cannot live in a separate file. Every one is a direct call into `deltachat::email::*` and the types are in `api/types/email.rs`, so on conflict re-place the block rather than replaying the diff. | [0012](adr/0012-rpc-and-cli.md) |
| `core/deltachat-jsonrpc/src/api/types/mod.rs` | `pub mod email;`. | Registers our types. | [0012](adr/0012-rpc-and-cli.md) |
| `core/src/mimefactory.rs` | Added a `cc` field to `MimeFactory`, a block merging extra Cc/Bcc recipients into the envelope and key set before the encryption branch, a `Cc` header in `render_headers`, and a `cc_header()` accessor. | Core emits no `Cc` at all and derives addressing from chat membership, so a composer had nothing to write to. Placed **before** the encryption branch on purpose: inside it, every Cc was silently dropped from unencrypted mail. | [0014](adr/0014-recipient-sets-on-the-wire.md) |

| `core/src/sql/migrations.rs` | Migration 170: `CREATE TABLE structured_data`. | Machine-readable data a message carried about itself, with the trust verdict computed at receive and never recomputed. | [0016](adr/0016-structured-email.md) |
| `core/src/sql/migrations.rs` | Migration 169: `CREATE TABLE held_msgs`, `trashed_msgs`, and the reserved `Trash` and `Holding` rows. | Contact gating and the recoverable trash. Both tables hold a **local** purge deadline that is deliberately never synced. | [0017](adr/0017-system-tags.md), [0018](adr/0018-contact-gating.md), [0019](adr/0019-recoverable-ephemeral-expiry.md) |
| `core/src/sql/migrations.rs` | Migration 171: renames the `Holding` label row to `Unverified`, carries the `ephemeral_trash_days` config value over to `trash_purge_days`, and drops `held_msgs.purge_at` with its index. | All three are **renames of data, not schema additions**, and each has a silent failure mode if skipped. The label is renamed in place so `msg_labels` rows keep pointing at it. `Config` is stored under its snake_case name, so an un-carried key drops the account back to `0` — destroy immediately. `purge_at` goes because the deadline is now `held_at` plus the current setting, computed at sweep time, and a stored deadline would be a second source of truth that outvoted the setting. | [0017](adr/0017-system-tags.md), [0018](adr/0018-contact-gating.md), [0019](adr/0019-recoverable-ephemeral-expiry.md) |
| `core/src/sql/migrations/migrations_tests.rs` | One test appended, `test_unverified_rename_migration`. | Migration 171 renames data three ways and each has a silent failure mode, so it is asserted rather than assumed. Uses upstream's own `STOP_MIGRATIONS_AT` harness to build a v170 database and migrate it. | [0018](adr/0018-contact-gating.md), [0019](adr/0019-recoverable-ephemeral-expiry.md) |
| `core/src/config.rs` | Added `Config::InboxGating`, `Config::UnverifiedTrashDays` and `Config::TrashPurgeDays` (named `EphemeralTrashDays` until v0.3.0), **all defaulting to upstream's behaviour** (`0`). Rewrote the `EphemeralDefaultSeconds` rationale, which described a destructive expiry that no longer exists. | eeemail turns all three on in `email::policy::apply_defaults` instead. Flipping the compile-time defaults broke eight upstream tests that assert a fired timer destroys the message and that a stranger's mail reaches the inbox — the same trade `ForceEncryption` already refused. Note `UnverifiedTrashDays` reads `0` as **never sweep**, not sweep at once, which is what makes upstream's default the do-nothing one. Upstream test churn: zero. | [0012](adr/0012-rpc-and-cli.md), [0018](adr/0018-contact-gating.md), [0019](adr/0019-recoverable-ephemeral-expiry.md) |
| `core/src/receive_imf.rs` | One more call, `email::structured::store`, in the existing best-effort block, after `gating::apply`. | Extracts structured data and freezes its trust verdict. After gating so the verdict agrees with where the message landed. Takes `imf_raw` as well as the parser, because `decoded_data` is empty unless something was decrypted. No new patch site. | [0016](adr/0016-structured-email.md) |
| `core/src/receive_imf.rs` | One more call, `email::autocrypt::adopt`, in the existing best-effort block, *outside* the per-message loop. | Learns the sender's advertised key so the next message to them can be encrypted. Outside the loop because the key belongs to the sender, not to any one of the ids a message produced. No new patch site. | [0021](adr/0021-autocrypt-key-contacts.md) |
| `core/src/receive_imf.rs` | One more call, `email::gating::apply`, in the existing best-effort block. | Holds mail from a sender who is neither verified nor known. Placed after `drain_pending` so a label synced from another device is already on the message when this classifies it. No new patch site: the block was already there. | [0018](adr/0018-contact-gating.md) |
| `core/src/contact.rs` | Two `email::gating::release` calls: one after the transaction in `ContactId::scaleup_origin`, one after the transaction in `mark_contact_id_as_verified`. | The two ways a sender becomes trusted, and therefore the two places their held mail must be let out. Best-effort. `release` re-checks each contact rather than trusting the call site, because origin is scaled up constantly and most scale-ups do not cross into trusted -- so if upstream moves these, re-place them at whatever the new choke points are and the behaviour is unchanged. | [0018](adr/0018-contact-gating.md) |
| `core/src/ephemeral.rs` | One best-effort `email::ephemeral::divert` call at the top of `delete_expired_messages`, before `select_expired_messages`. | Expiry moves a message to `Trash` for a recoverable window instead of destroying it. **The ordering is the patch**: `divert` clears `ephemeral_timestamp` on what it takes, so the select immediately below no longer sees those rows. If upstream restructures this function, the call must stay ahead of the select or expiry becomes destructive again, silently. `delete_device_after` expiries are deliberately left for core to destroy. | [0019](adr/0019-recoverable-ephemeral-expiry.md) |
| `core/src/sql.rs` | `email::gating::sweep` and `email::ephemeral::purge` added to the eeemail housekeeping block, in that order. | The two deadlines. `sweep` only **moves** mail, into `Trash` with a deadline of its own, so it runs first and what it sweeps gets a full trash window rather than being destroyed in the same pass. Only `ephemeral::purge` destroys, through `MsgId::trash`, and it runs before reference collection so freed blobs are reclaimed in the same pass. | [0018](adr/0018-contact-gating.md), [0019](adr/0019-recoverable-ephemeral-expiry.md) |
| `core/deltachat-jsonrpc/src/api.rs` | Four more imports and a second contiguous block of thin methods at the end of `impl CommandApi`, covering tags, gating and the trash. | Same constraint as the first block: `yerpc`'s `#[rpc]` applies to one `impl`. Re-place the block rather than replaying the diff. | [0017](adr/0017-system-tags.md), [0018](adr/0018-contact-gating.md), [0019](adr/0019-recoverable-ephemeral-expiry.md) |

| `core/Cargo.toml` | Added `chacha20poly1305 = "0.10"`. | Per-blob AEAD. Already in the lockfile via `rpgp`, so this promotes a transitive dependency to a direct one rather than adding a new one. | [0020](adr/0020-blobdir-encryption.md) |
| `core/src/config.rs` | Added `Config::BlobEncryption` (default `0`). | Opt-in encryption of the blobdir, off by default. | [0020](adr/0020-blobdir-encryption.md) |
| `core/src/context.rs` | Two entries in the `get_info` map: `unverified_trash_days` and `trash_purge_days`. | What `eeemail-cli info` reports. Replaced a single `ephemeral_trash_days` entry when the key was renamed. | [0018](adr/0018-contact-gating.md), [0019](adr/0019-recoverable-ephemeral-expiry.md) |
| `core/src/context.rs` | Added `inbox_gating` and `blob_encryption` to the `get_info` map. | Upstream's `test_get_info_completeness` requires every `Config` key to appear in `get_info` or be explicitly skipped. | [0018](adr/0018-contact-gating.md), [0020](adr/0020-blobdir-encryption.md) |
| `core/src/blob.rs` | One `email::blobcrypt::protect` call in `create_and_deduplicate`, immediately after the `rename` and **after** the hash. | Encrypts a blob once it is named. **The position is the patch**: the hash above it is taken over the *plaintext*, because hashing ciphertext with a random nonce would give every copy of the same message a different name and silently double disk usage. Moving this call above the hash compiles, passes most tests, and breaks deduplication. Best-effort, so a blob that fails to encrypt is still stored. | [0020](adr/0020-blobdir-encryption.md) |
| `core/src/tools.rs` | `read_file` reads through `email::blobcrypt::read` instead of `fs::read`. | Transparent decryption. A file without the `EEEBLOB1` magic is returned untouched, so this is correct whether encryption is on, off, or part-way through a migration. | [0020](adr/0020-blobdir-encryption.md) |
| `core/src/mimefactory.rs` | Two attachment reads switched to `email::blobcrypt::read`. | An attachment encrypted at rest must still reach the wire as plaintext. | [0020](adr/0020-blobdir-encryption.md) |
| `core/src/config.rs` | Self-avatar read switched to `email::blobcrypt::read`. | Same. | [0020](adr/0020-blobdir-encryption.md) |
| `core/src/contact.rs` | Avatar read switched to `email::blobcrypt::read`. | Same. | [0020](adr/0020-blobdir-encryption.md) |
| `core/src/message.rs` | HTML-part and vCard reads switched to `email::blobcrypt::read`. | Same. | [0020](adr/0020-blobdir-encryption.md) |
| `core/src/qr_code_generator.rs` | Two avatar reads switched to `email::blobcrypt::read`. | Same. | [0020](adr/0020-blobdir-encryption.md) |
| `core/src/email/vault.rs` | `protection` measures the blobdir through `blobcrypt::cleartext_bytes` instead of hardcoding `blobs_encrypted: false`. | Ours, not upstream's, but listed because the honesty property depends on it: the value is measured from the files rather than read off the setting, so an interrupted migration reports `partial` rather than a half-truth. | [0015](adr/0015-at-rest-and-backup.md), [0020](adr/0020-blobdir-encryption.md) |

| `core/src/message.rs` | `save_file` reads through `email::blobcrypt::read` and writes the plaintext, instead of copying the blob byte-for-byte with `io::copy`. | Saving an attachment is the one path on which a stored blob leaves the blobdir without being interpreted, and it was missed when every other read path was converted. With `BlobEncryption` on, a plain copy hands the user an `EEEBLOB1` container named `report.pdf`. `create_new(true)` is preserved -- `imex/transfer.rs` asserts that saving over an existing file fails -- and the write is flushed, because tokio's `File` buffers and dropping it loses the write. Guarded by `blobcrypt_tests::test_saving_an_attachment_writes_plaintext`. | [0020](adr/0020-blobdir-encryption.md) |
| `core/src/config.rs` | Added `Config::SubjectInBody`, **defaulting to upstream's behaviour** (`1`). | Upstream prepends the subject into the body text of classic mail, which suits a chat bubble with no subject line and corrupts the body of a message an email client shows the subject of separately. Keeping upstream's compile-time default is what makes this a three-line change: roughly 37 upstream test assertions expect the prepended form, and patching them would be a conflict on every merge forever. `email::policy::apply_defaults` turns it off for eeemail accounts instead. Upstream test churn: zero. | [0008](adr/0008-email-message-model.md), [0012](adr/0012-rpc-and-cli.md) |
| `core/src/mimeparser.rs` | One extra condition on the subject-prepend block in `parse_headers`, reading `Config::SubjectInBody`. | The gate for the above. `parse_headers` already takes `&Context`, so no signature changes. The neighbouring block that uses the subject *as* the body when a message has no text part is deliberately untouched: that one is desirable. | [0008](adr/0008-email-message-model.md) |
| `core/src/context.rs` | Added `subject_in_body` to the `get_info` map. | Upstream's `test_get_info_completeness` requires every `Config` key to appear in `get_info` or be explicitly skipped. | [0008](adr/0008-email-message-model.md) |

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
| `core/src/email/compose.rs` | Per-message recipient sets on the wire: Cc and Bcc | [0014](adr/0014-recipient-sets-on-the-wire.md) |
| `core/src/email/autocrypt.rs` | Key-contact from an incoming `Autocrypt:` header, so opportunistic encryption can start | [0021](adr/0021-autocrypt-key-contacts.md) |
| `core/src/email/structured.rs` | SML / Schema.org-for-Email extraction with a trust verdict | [0016](adr/0016-structured-email.md) |
| `core/src/email/vault.rs` | At-rest protection reporting | [0015](adr/0015-at-rest-and-backup.md) |
| `core/src/email/backup.rs` | Encrypted backup with staleness tracking | [0015](adr/0015-at-rest-and-backup.md) |
| `core/deltachat-jsonrpc/src/api/types/email.rs` | JSON-RPC types for the email layer | [0012](adr/0012-rpc-and-cli.md) |
| `cli/` | Headless driver for development and integration tests | [0012](adr/0012-rpc-and-cli.md) |
| `desktop/` | Tauri v2 shell and TypeScript frontend | [0013](adr/0013-desktop-ui.md) |

Conflict guidance for the gating and trash hooks: the `ephemeral.rs` one is the
only patch in this ledger whose **position** carries meaning rather than its
content. `divert` must run before `select_expired_messages`, because it works by
clearing the column that select reads. A merge that keeps the call but moves it
after the select compiles, passes most tests, and silently restores the
destructive behaviour ADR 0019 exists to remove. It is covered by
`email::ephemeral::ephemeral_tests::test_expiry_clears_the_timer_so_core_does_not_destroy_it`.

The two `contact.rs` hooks are the opposite: their position is incidental. They
are at the two choke points through which a contact becomes known or verified
today. If upstream moves those, re-place the calls at the new ones; `release`
re-checks trust itself, so calling it too often is wasteful and never wrong.

Conflict guidance for the at-rest hooks: the read redirections are mechanical
and interchangeable -- each is one line, `fs::read(path)` becoming
`email::blobcrypt::read(context, &path)`, and `read` is transparent, so a
redirection that gets lost in a merge degrades to "this blob reads as
ciphertext" rather than to a crash. The `blob.rs` write hook is the one that
matters: it must stay **after** the hash and **after** the rename. There is a
test, `blobcrypt_tests::test_dedup_still_hashes_plaintext`, and it is the only
thing standing between a merge and a silently doubled blobdir.

Conflict guidance for encryption, which is the subtlest thing in this fork.
Upstream `v2.59` decides encryption by *contact type* — `Chat::is_encrypted`
reads the contact row's `fingerprint` — and mints such a contact only from a
signed message or SecureJoin. eeemail depends on three things holding together
on top of that, and a merge can break any of them without failing to compile:

* `email::autocrypt::adopt` creates the key-contact an `Autocrypt:` header
  implies, which is the only reason opportunistic encryption can ever start.
  Guarded by `autocrypt_tests::test_mail_after_an_autocrypt_header_is_encrypted`.
* `email::compose::send` addresses the **key**-contact when one exists. Resolving
  with `Contact::add_or_lookup` alone returns the address-contact and sends
  cleartext to someone whose key we hold, including someone verified by QR.
  Guarded by `compose_tests::test_a_verified_contact_gets_an_encrypted_chat`.
* `email::gating::is_trusted` decides trust per *person*, across every contact
  row sharing an address. The same correspondent is routinely two rows, and
  asking only about the row a message arrived on holds the first encrypted reply
  from everyone the user has written to. Guarded by
  `gating_tests::test_an_encrypted_reply_from_someone_you_wrote_to_is_not_held`.

If upstream reinstates Autocrypt-derived contacts, `adopt` becomes redundant and
should be deleted rather than left to race with it.

Conflict guidance for structured email: `email::structured` re-walks the raw
MIME with `mailparse` instead of reading `MimeMessage::parts`, because core
drops an `application/*` part that has no filename as a "Missing attachment"
(`mimeparser.rs`) — which is exactly the shape an SML part has. If a merge makes
`parts` carry those, the walk can be simplified; until then, reading `parts`
would silently find nothing. Guarded by
`structured_tests::test_a_machine_readable_part_is_extracted`, and the
`Missing attachment` warning is asserted in the tests so that the day it stops
being logged is a test failure rather than a silent behaviour change.
