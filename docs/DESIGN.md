# eeemail — Design

## Context

**The problem.** Delta Chat solved the hardest parts of usable email encryption: opportunistic key exchange (Autocrypt), a QR-code contact-verification protocol that resists active MITM attacks (SecureJoin), ephemeral messages, multi-device sync, and encrypted backup — all riding on plain SMTP/IMAP with no new server stack. But Delta Chat presents this as a *chat* client, and upstream has no interest in a traditional email UI.

**What eeemail is.** A classic email client — subjects, CC/BCC, threads, attachments, search, drafts, labels — built on Delta Chat's encryption engine.

**The transport model.** IMAP/SMTP are *transport only*, not storage. We use them because they're standard and well-tested servers already exist, and because a user can reuse their existing mail platform. A single IMAP folder is watched; messages are downloaded, decrypted, stored locally, and removed from the server. Decrypted content is never written back to the server. This is the chatmail relay model: Postfix accepts and relays, Dovecot spools to Maildir until download, `chatmail-expire` and quota rules clear the rest, `filtermail` rejects unencrypted mail at the perimeter. The server is a spool; **the local database is the mailbox.**

**Why we fork rather than rebuild.** An earlier draft of this plan recommended a new engine, on the grounds that `chatmail/core` had stripped folder support, raw-MIME retention and multi-folder sync. That reasoning was wrong for this project: single-folder IMAP-as-transport is precisely what we want. Core already implements Autocrypt, SecureJoin, ephemeral messages, MDN read receipts, multi-device sync, encrypted backup, and even WebRTC calls with STUN/TURN/ICE. Reimplementing those would be many months of protocol work that already exists and is interop-tested in production.

The remaining gap is genuinely only the **email-client layer**, and that is what we build.

### Decisions taken (confirmed with the user)

| Decision | Choice |
|---|---|
| Core strategy | **Fork `chatmail/core`**; build the email-client layer on top |
| License | Relicense repo from Unlicense to **MPL-2.0** (matches core) |
| Storage | Local DB is canonical; decrypted content never returns to the server |
| Raw MIME | Retained for a **configurable** period (short by default, up to indefinite) |
| IMAP | Single folder, transport only. No folder tree. |
| Organization | **Labels/tags + archive**, local-only, synced across the user's devices |
| Encryption | **Opportunistic by default** (original Delta Chat behavior), user-settable stricter (E2E-only) or more lenient |
| Ephemeral + read receipts | **On by default**, globally disableable, with per-contact overrides |
| First client | Desktop: Tauri v2 + web frontend |
| Server | Ship a Postfix/Dovecot template adapted from chatmail relay's config, with **traditional `user@domain` accounts** |
| Excluded | Relay create-on-login/random-address provisioning, decentralized groups & channels, verified groups, webxdc, iroh peer channels |
| Later | STUN-based device sync, WebRTC calls, encrypted cloud backup |

---

## What we inherit from core (day one, free)

Forking means these arrive working and interop-tested rather than as implementation phases:

| Capability | Core module |
|---|---|
| Autocrypt key exchange + gossip + Setup Message | `aheader.rs`, `e2ee.rs`, `keyupdate.rs` |
| SecureJoin QR contact verification | `securejoin/`, `qr.rs`, `qr_code_generator.rs`, `token.rs` |
| OpenPGP (rPGP), PGP/MIME, RFC 9788 protected headers | `pgp.rs`, `decrypt.rs`, `key.rs` |
| IMAP sync, IDLE, UID/UIDVALIDITY tracking (`imap_sync` table, per-transport) | `imap/`, `scheduler.rs` |
| SMTP submission with queue | `smtp/`, `transport.rs` |
| MIME parse/build | `mimeparser/`, `mimefactory/`, `simplify.rs`, `dehtml.rs` |
| Ephemeral message timers | `ephemeral.rs` |
| Read receipts (MDN) | `MdnsEnabled` config + `receive_imf/` |
| Multi-device sync (`BccSelf` + sync messages) | `sync.rs` |
| Encrypted backup + device-to-device transfer | `imex/` |
| WebRTC calls with STUN/TURN/ICE (ICE servers from IMAP METADATA) | `calls.rs` |
| Provider autoconfig database | `provider.rs` |
| Multi-transport (multiple relays/servers per profile) | `transport.rs`, `addTransport` RPC |
| JSON-RPC API + stdio server | `deltachat-jsonrpc/`, `deltachat-rpc-server/` |

---

## What we must build (the actual work)

Verified against current `chatmail/core` `main`:

1. **Raw MIME retention with configurable expiry.** Core does *not* store raw MIME — `SaveMimeHeaders` was removed and `message.rs` retains only `rfc724_mid` and `mime_in_reply_to`. We add a blob-backed raw store plus a retention policy. Needed for view-source, signature re-verification, faithful reply/forward quoting, and export. Default retention is short (transport + reply window); the user can extend it to indefinite.

2. **Per-message subject as a first-class field.** Core derives chat names from subjects and only exposes subject-setting on its cleartext "new email" action. We need subject preserved on receive and settable on send, including for encrypted mail (carried in protected headers).

3. **Recipients decoupled from chat membership.** This is the deepest model change. In core a message goes to a *chat*; in email a message goes to a *recipient set*. We add explicit To/CC/BCC lists per message.

4. **Threading.** JWZ threading over `References`/`In-Reply-To` to build conversation views, replacing chat-assignment as the primary grouping.

5. **Labels/tags + archive.** New local-only tables, synced between the user's devices over core's existing sync-message channel.

6. **Server retention policy.** Configurable: delete-after-download (default), keep N days, or never delete — the last being the opt-in coexistence mode for users who also run Thunderbird or webmail against the same account.

7. **Three-mode encryption strictness.** *Strict* (E2E only; maps onto core's existing `ForceEncryption`), *Opportunistic* (default), *Lenient*. Plus per-contact overrides.

8. **Ephemeral + MDN policy layer.** Both on by default with per-contact overrides; read receipts enabled for verified contacts in the address book.

9. **The email UI itself.** Message list, threaded reading pane, composer with subject/CC/BCC/attachments, label sidebar, search, contact management with verification badges.

---

## Architecture

```
eeemail/
├── core/                 # fork of chatmail/core (MPL-2.0), upstream tracked as a git remote
│   └── src/
│       ├── email/        # NEW — our email-client layer, isolated for merge sanity
│       │   ├── rawmime.rs    # raw MIME blob store + retention/expiry
│       │   ├── recipients.rs # To/CC/BCC sets decoupled from chat membership
│       │   ├── threading.rs  # JWZ threading over References/In-Reply-To
│       │   ├── labels.rs     # labels/tags + archive
│       │   └── policy.rs     # encryption strictness, retention, MDN/ephemeral defaults
│       └── ...           # upstream modules, minimally patched
├── rpc/                  # extends deltachat-jsonrpc with the email methods
├── cli/                  # headless driver for dev + integration tests
├── desktop/              # Tauri v2 shell + TypeScript frontend
└── server/               # Postfix/Dovecot deployment template (see below)
    ├── deploy/           #   adapted from chatmail/relay cmdeploy
    └── compose/          #   docker-compose variant used by CI
```

**Fork discipline — this is what determines whether the fork stays maintainable.** Keep `chatmail/core` as a git remote and merge periodically, forking from tagged releases rather than `main`. Concentrate every addition in `core/src/email/` and touch upstream files as narrowly as possible (ideally single call-sites and hook points). Record each upstream-file patch in [`fork-patches.md`](fork-patches.md) with its rationale, so a merge conflict can be resolved by someone who wasn't there — `scripts/check-fork-patches.sh` fails CI if you don't.

**Do not rip out unwanted features in Phase 0.** Group machinery, `webxdc.rs` and `peer_channels.rs` are woven through `receive_imf/` and `chat/`; aggressive early deletion means fighting the compiler instead of building. Disable them at the API layer first behind cargo features, and delete incrementally once our layer is stable. The measured site-by-site inventory is in [`out-of-scope.md`](out-of-scope.md).

### Storage additions

```sql
raw_mime(msg_id, blob_hash, size, received_at, expires_at)  -- NULL expires_at = keep forever
msg_recipients(msg_id, addr, kind)                          -- kind: to | cc | bcc
threads(id, root_rfc724_mid, subject_norm, last_activity)
msg_threads(msg_id, thread_id)
labels(id, name, color, is_system)                          -- Inbox/Archive/Sent/Drafts/Trash + user labels
msg_labels(msg_id, label_id)
contact_policy(contact_id, mdn_enabled, ephemeral_secs, encryption_mode)
```

Labels rather than folders means `msg_labels` is many-to-many by construction, and archive is just the removal of the Inbox label — which fits a threaded conversation view better than a tree would.

---

## The server side: `server/` — a chatmail-derived Postfix/Dovecot template

We want chatmail's **server configuration**, not its **relay addressing model**. Chatmail relays hand out random 9-character addresses via create-on-login provisioning; eeemail uses traditional `user@domain` accounts, administratively provisioned. But the hardened spool configuration underneath is exactly right for transport-only mail, and it gives us a reproducible integration-test target as a side effect.

`server/` ships a deployment template adapted from [`chatmail/relay`](https://github.com/chatmail/relay)'s `cmdeploy` tree, plus a docker-compose variant for CI.

**Adopt:**

| Piece | What it gives us |
|---|---|
| **Postfix** config | Strict-TLS-only, ports 25/587/465, per-user rate limiting (`max_user_send_per_minute`, burst), message size caps |
| **Dovecot** config | Maildir spool, quota enforcement, IMAP connection limits, IMAP `METADATA` support |
| **OpenDKIM** | DKIM signing on outbound; inbound requires a valid signature with `d=` matching the `From:` domain |
| **filtermail** | Perimeter enforcement that unencrypted mail neither enters nor leaves — the server-side counterpart to our *strict* encryption mode |
| **expire** | `delete_mails_after` (20d), `delete_large_after` (7d, >200k) — spool hygiene, since clients hold the real mailbox |
| **unbound / acmetool / DNS helper** | Local resolver, ACME certificates, and DNS record generation for MX/SPF/DKIM/DMARC |
| **`METADATA` for ICE servers** | Core's `calls.rs` already reads STUN/TURN servers from IMAP `METADATA` — this is what makes Phase 8 calls and device sync work |

**Drop:**

- `doveauth` create-on-login semantics and `newemail.py` random-address generation — replaced by ordinary account provisioning with traditional addresses.
- `username_min_length`/`username_max_length` pinned to 9 — real addresses, not tokens.
- `delete_inactive_users_after` (90d, deletes entire mailboxes) — off by default. Acceptable for disposable relay identities, unacceptable for a person's real address.
- The iroh relay service (decentralized channels, out of scope).
- Push notification forwarding to `notifications.delta.chat` — a third-party dependency; optional and self-hosted if we want it later.

**Important:** this template is a convenience for testing and self-hosting, not a requirement. eeemail must work against any standards-compliant IMAP/SMTP provider, including a user's existing mailbox.

---

## Implementation phases

Each phase ends runnable and testable through `cli/`.

**Phase 0 — Fork and foundation.** *(complete)*
Vendored `chatmail/core` at **`v2.59.0`** (`e322fdf`) into `core/` via `git subtree`, with upstream as a git remote and a documented merge policy in [`fork-patches.md`](fork-patches.md). Relicensed to MPL-2.0 (`LICENSE` + `NOTICE`). CI at `.github/workflows/ci.yml` runs rustfmt, clippy, MSRV check and the test suite; `scripts/check-fork-patches.sh` mechanically enforces that every patched upstream file is recorded in the ledger. Out-of-scope features declared as cargo features with a measured removal inventory in [`out-of-scope.md`](out-of-scope.md).

*Gate met:* `cargo nextest run --workspace` — **1153 passed, 0 failed** (145s); doctests pass; `cargo fmt --check` clean. Note that `cargo test` fails on this green tree, because upstream's clock mock is a process-global whose shift accumulates — see [`testing.md`](testing.md).

**Phase 0.5 — Server template.**
`server/`: adapt chatmail relay's Postfix/Dovecot/OpenDKIM/filtermail configuration to traditional account provisioning, drop the relay-specific pieces listed above, and produce a docker-compose variant. This lands early because every later phase's integration tests run against it. *Gate:* `docker compose up` yields a working E2EE-enforcing mail server that a Delta Chat client can also use, and CI can provision accounts against it.

**Phase 1 — Raw MIME retention.** *(engine complete)*
`core/src/email/rawmime.rs`: retention over core's existing content-addressed `BlobObject::create_and_deduplicate_from_bytes`, config key `raw_mime_retention_days` (0 = off, N days, negative = forever; default 30), migration 164, expiry wired into core's housekeeping *before* reference collection so freed blobs are reclaimed in the same pass. Hooked into `receive_imf_inner`'s success exit and `create_send_msg_jobs`.

Housekeeping also drops raw MIME whose message no longer exists, added in Phase 2: without it a deleted message kept its original bytes until expiry, which under `forever` meant permanently.

*Gate met:* 15 tests in `email::rawmime::rawmime_tests`, including byte-identical round-trip, expiry on schedule, `forever` never expiring, housekeeping keeping retained blobs and reclaiming expired ones, and dedup of identical messages. Full suite 1158/1158.

*Deferred:* the CLI (`show --raw`, `retention set`) moves to Phase 6, where `cli/` is actually built; adding a crate here would duplicate that work. Server retention policy (delete-after-download / keep N days / never) is still outstanding and moves to Phase 4, next to the encryption-policy work it shares a settings surface with.

**Phase 2 — Email message model.** *(engine complete)*
`core/src/email/recipients.rs` + `core/src/email/threading.rs`, migration 165. Per-message To/Cc/Bcc sets in `msg_recipients`, taken from `MimeMessage::merge_headers` so protected headers win over the outer ones; Bcc written only by the send path. Reference-chain threading in `threads`/`msg_threads`/`thread_refs`, with grouping persisted and tree shape derived on demand. Both hooked into the same two sites as Phase 1. See [ADR 0008](adr/0008-email-message-model.md).

Per-message subject turned out to need **verification, not implementation**: `msgs.subject` already exists, is populated on receive, and `Message::set_subject` already reaches the wire. Tests pin all three so a merge cannot quietly remove them.

*Gate met:* 11 tests in `email::recipients::recipients_tests` and 19 in `email::threading::threading_tests`, including To/Cc kept apart in header order, protected recipients used for encrypted mail, an incoming `Bcc` header ignored, out-of-order arrival, fragment merging, reference cycles, and a 50 000-deep tree that must build and drop without overflowing the stack. Full suite 1188/1188.

*Deferred:* sending **to** an arbitrary recipient set — a `Cc` naming someone outside the conversation — needs encryption keys resolved for addresses that may have none, so it lands with Phase 4 rather than here. The wire format also has no `Cc` today: `MimeFactory` derives addressing from chat membership. Interop against Thunderbird and a real-world mailbox import belongs with that work.

**Phase 3 — Organization and search.** *(engine complete)*
`core/src/email/labels.rs` + `core/src/email/search.rs`, migration 166. Labels in `labels`/`msg_labels`, synced between the user's devices by *name* over core's sync channel via one new `SyncData::EmailLabel` variant. Search over body, subject, recipients and labels as a separate entry point, leaving upstream's `search_msgs` untouched. See [ADR 0009](adr/0009-labels-and-search.md).

Two decisions worth knowing. **Archive is the presence of a reserved label, not the absence of an Inbox label** — the inverse of ADR 0005's wording, because every hook we install is best-effort, and an absence-based inbox would make a message vanish from both views if its hook failed. **Label changes that arrive before their message are parked, not dropped**, in `pending_msg_labels` and drained by the receive hook; core's own sync handlers drop in that case, which is tolerable for a deletion and not for a label.

*Gate met:* 22 tests in `email::labels::labels_tests` and 10 in `email::search::search_tests`, including case-insensitive label identity, system labels refusing rename and delete, out-of-order sync settling on the latest intent, a sync item never echoing one back, label names never appearing in cleartext on the wire, and search finding a message by a subject that upstream's `search_msgs` cannot see.

*Deferred:* Sent/Drafts/Trash are presented as system labels by the UI but must be **derived** from `MessageState` and `chat_id`, which core already owns. Storing them would create a second source of truth.

**Phase 4 — Encryption policy and server retention.** *(engine complete)*
`core/src/email/policy.rs`, migration 167. Strict/opportunistic/lenient with per-contact overrides in `contact_policy`, composing toward the **strictest**. Enforcement needs no change to the send path: `Param::GuaranteeE2ee` and `Param::ForcePlaintext` already exist. Server retention (delete-after-download / keep N days / never), deferred here from Phase 1. Per-message encryption and signature state via `MessageCrypto`. See [ADR 0006](adr/0006-encryption-policy.md) and [ADR 0010](adr/0010-server-retention.md).

**The silent recipient drop.** When a message goes out encrypted, `mimefactory.rs` drops recipients whose key is missing from the envelope but leaves them in the `To` header — in **strict mode too**, not just opportunistic. In a group chat that is defensible; in email you address three people, one never receives it, and nothing says so. We surface it (`msg_undelivered`, plus a warning) rather than change a behaviour upstream's tests depend on.

**Server retention is never retroactive**, so pointing eeemail at an existing mailbox cannot destroy mail that was already there — the same principle as raw-MIME retention in Phase 1.

*Gate met:* 23 tests in `email::policy::policy_tests`. Full suite 1243/1243.

*Deferred:* interop against a real Delta Chat client needs a second live client and is integration work for Phase 6's CLI. Sending to an arbitrary recipient set (a `Cc` naming someone outside the conversation), carried over from Phase 2, is **still outstanding**: `MimeFactory` emits no `Cc` header at all and derives both addressing and the encryption key set from chat membership. It is a larger change than the whole of Phase 4 and is called out separately rather than half-done.


**Phase 5 — Ephemeral and read receipts.** *(engine complete)*
`core/src/email/receipts.rs`, migration 168. A three-state read-receipt policy — never / verified-and-known contacts only / always — with per-contact overrides, plus a default ephemeral timer with per-contact overrides applied on the first message to a conversation. See [ADR 0011](adr/0011-receipts-and-ephemeral.md).

**Read receipts are on by default already**; what was missing was *who*. The verified-only setting is what makes "if a contact is verified and in the address book, they get read receipts" expressible as a policy. A global off is a **hard off**, beating the policy and every override — the reverse would be a privacy regression.

**Ephemeral machinery is complete but ships disabled**, which is a deliberate departure from "on by default" that should be reviewed. Ephemeral deletion removes the local copy, and the local store is the only durable copy of the mailbox ([ADR 0004](adr/0004-local-store-and-raw-mime.md)), so a non-zero default silently destroys mail. No duration was specified. Turning it on is one config value and every test covers the on case.

*Gate met:* 21 tests in `email::receipts::receipts_tests`. Full suite 1264/1264.

**Phase 6 — RPC and CLI.** *(complete)*
~30 methods added to `deltachat-jsonrpc` covering raw source, recipients, threads, labels, search, encryption policy, server retention and read receipts, with types in `deltachat-jsonrpc/src/api/types/email.rs`; TypeScript bindings generate. `cli/` is a headless driver: one command per invocation, JSON on stdout, non-zero exit on error. See [ADR 0012](adr/0012-rpc-and-cli.md).

**eeemail's defaults are applied at setup, not as compile-time defaults.** Flipping `ForceEncryption`'s default to reach ADR 0006's opportunistic default fails 22 upstream tests that assert upstream's policy — 22 permanent merge conflict points for a value that only has to be written once. `email::policy::apply_defaults` does it instead, never touching a configured account (`ForceEncryption` is device-synced, so writing it would push a weaker policy to the user's other clients) and never overwriting an explicit choice. Upstream test churn: **zero**.

Threads come back **flat** — `(msgId, parentMsgId, depth)` in display order — because `typescript_type_def` cannot express a recursive type, and a flat list is what a reading pane renders anyway.

*Gate met:* full suite 1267/1267; TypeScript bindings generate; the CLI exercised end-to-end against a real account.

**Phase 7 — Desktop UI.** *(reading client complete; composer and setup outstanding)*
`desktop/`: a Tauri v2 shell over the RPC surface, plus a TypeScript frontend. Message list, threaded reading pane, label sidebar, search, and per-message encryption/verification/undelivered/source state. See [ADR 0013](adr/0013-desktop-ui.md).

**The shell is one JSON-RPC pipe, not a Tauri command per method.** The RPC surface is a couple of hundred methods and grows every phase; mirroring each would mean writing every signature three times. Requests go in through `rpc_send`, and everything coming back — responses *and* engine events — is emitted as one `rpc-message` stream, the same shape `deltachat-rpc-server` has over stdio.

**Untrusted content has two independent barriers**, and the redundancy is the point: the window CSP allows no remote origins, *and* the renderer strips remote references and reports that it did. A single remote image is a read receipt the sender gets without consent, plus the user's IP. HTML mail renders only inside an iframe with a bare `sandbox` attribute — no scripts, no forms, no navigation, `null` origin — never in the app document. CI asserts all of this against the built bundle.

*Gate met:* frontend typechecks and builds; the Rust shell compiles clean under `-Dwarnings`; the app launches under Xvfb, opens the account store and enters its event loop.

*Outstanding:* the composer, account setup, contact management with verification badges, and QR display/scan. The composer in particular is blocked on the same thing Phases 2 and 4 deferred — `MimeFactory` emits no `Cc` header and derives addressing from chat membership — so building a To/Cc/Bcc composer before that lands would produce a form whose fields the engine ignores.

**Phase 8 — At-rest protection, backup, calls.** *(engine complete)*
`core/src/email/vault.rs` + `core/src/email/backup.rs`. See [ADR 0015](adr/0015-at-rest-and-backup.md).

**Database encryption is reported as partial, and that is the finding.** Upstream deprecated it in 2025-11 because "Db encryption does nothing with blobs". For Delta Chat that leaves attachments in the clear; for eeemail it leaves attachments **and the raw MIME of every retained message** — complete original messages — in the blobdir beside an encrypted database. Raw MIME retention is our own addition, so we widened that gap. `vault::protection` therefore reports `blobs_encrypted` (always false), the cleartext byte count, and a summary sentence naming both, so a settings screen cannot claim more than is true.

**Backup requires a passphrase** where core's `imex` accepts an optional one — an unencrypted backup is a copy of the whole mailbox in the clear, and must not be reachable by leaving a field blank. Staleness is reported, not scheduled: an automatic backup needs a destination, and every such place is a decision with consequences the user has to make.

**WebRTC calls and ICE were already exposed** by `deltachat-jsonrpc`; nothing to build.

*Gate met:* 11 tests in `email::vault` and `email::backup`, including a full backup round trip and a wrong-passphrase refusal.

*Not implemented:* cloud upload (needs credentials, a provider API and a per-provider threat model — none buildable honestly without picking one), blobdir encryption, and STUN-based direct device sync.

**Phase 9 — Recipient sets on the wire.** *(complete)*
`core/src/email/compose.rs`. Closes the gap Phases 2, 4 and 7 each deferred: core derives the `To` header, the envelope *and* the key set from chat membership, and emits no `Cc` at all. A message now carries extra recipients of its own. See [ADR 0014](adr/0014-recipient-sets-on-the-wire.md).

Every Cc/Bcc address resolves to a `ContactId`, which is what makes "do we have a key for them?" *the same question* it is for a chat member — so [ADR 0006](adr/0006-encryption-policy.md)'s policy applies unchanged instead of growing a second path. Bcc reaches the envelope and the key set and **no header**.

*Gate met:* 11 tests in `email::compose::compose_tests`. Full suite 1289/1289.

Three real bugs surfaced and were fixed: extra recipients merged inside the encryption branch dropped every Cc from unencrypted mail; `record_undelivered` only compared the `To` header, so a copied recipient dropped for want of a key was invisible; and the send path rebuilt `msg_recipients` from the `To` header, erasing the composer's Cc and Bcc.

---

## Verification

**Per phase.** Unit tests in-crate; upstream's existing test suite must stay green after every merge. Integration tests run against `server/compose` — our own Postfix + Dovecot template — driven through `cli/`. `cargo clippy -- -D warnings` and `cargo fmt --check` in CI. Additionally test against at least one mainstream provider (Gmail or Fastmail) so we never quietly become dependent on our own server's quirks.

**Protocol interop — the tests that actually matter.**
- **Autocrypt:** exchange mail with Delta Chat and with Thunderbird; keys learned both directions, replies encrypt automatically.
- **SecureJoin:** complete Setup-Contact against a real Delta Chat client, scanning in each direction; both sides reach verified.
- **Classic email:** a subject-bearing, CC'd, attachment-carrying message from eeemail renders and threads correctly in Thunderbird and Gmail — and the reverse.

**Data-safety regressions — these must never land.**
- **No plaintext leaves the device:** capture all IMAP APPEND and SMTP traffic during a full sync-and-send cycle and assert no decrypted body or subject appears in it, including in `BccSelf` self-copies.
- **Retention is honored:** raw MIME past its expiry is gone from the blob store; a message whose server retention has elapsed is gone from the server; and in coexistence mode ("never delete") the server mailbox is byte-for-byte unchanged after a full sync.

**End-to-end.** Phase 2: `cli` sends mail that arrives correctly in Thunderbird. Phase 7: run the Tauri app against a real account and read, compose, label and search through the UI.

---

## Sources

- [chatmail/core](https://github.com/chatmail/core) — module, `Config`, `imap.rs`, `message.rs`, `calls.rs` inspection
- [chatmail/relay](https://github.com/chatmail/relay) and [relay technical overview](https://chatmail.at/doc/relay/overview.html) — Postfix/Dovecot/filtermail/OpenDKIM architecture, `cmdeploy` tree, `chatmail.ini` defaults, expiry and quota policy
- [Delta Chat V2: encryption changes](https://delta.chat/en/2025-08-04-encryption-v2)
- [Zero metadata / RFC 9788 header protection](https://delta.chat/en/2026-03-31-zero)
- [SecureJoin specification v0.20.0](https://securejoin.delta.chat/en/latest/)
- [Autocrypt Level 1.1](https://docs.autocrypt.org/level1.html)
- [RFC 8098 — Message Disposition Notification](https://datatracker.ietf.org/doc/html/rfc8098)
- [Delta Chat on local data encryption](https://support.delta.chat/t/password-or-pin-protection-for-opening-app-local-data-encryption/1803)
- [Tauri 2.0](https://v2.tauri.app/blog/tauri-20/)
