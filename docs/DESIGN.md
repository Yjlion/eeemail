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

**Phase 1 — Raw MIME retention.**
`email/rawmime.rs`: content-addressed blob store, retention config (short default → indefinite), expiry in core's existing housekeeping job. Hook the store into `receive_imf` and the send path. Server retention policy (delete-after-download / keep N days / never). CLI: `show --raw`, `retention set`. *Gate:* a message survives round-trip with byte-identical raw MIME, and expires on schedule.

**Phase 2 — Email message model.**
`email/recipients.rs` + `email/threading.rs`. Per-message subject on receive and send; To/CC/BCC recipient sets; JWZ threading. Compose API takes a subject and a recipient set rather than a chat. *Gate:* send a subject-bearing, CC'd message that threads correctly in Thunderbird, and thread an imported real-world mailbox correctly.

**Phase 3 — Organization and search.**
`email/labels.rs`: labels/tags, archive, system labels; sync across devices over core's sync messages. Extend core's `search_msgs` to cover subject, recipients and labels.

**Phase 4 — Encryption policy.**
`email/policy.rs`: strict / opportunistic / lenient, built on `ForceEncryption`, with per-contact overrides. Per-message encryption and signature state surfaced in the model. *Gate:* SecureJoin and Autocrypt still interoperate with a real Delta Chat client in both directions.

**Phase 5 — Ephemeral and read receipts.**
Both on by default; global toggle; per-contact overrides; read receipts on for verified address-book contacts.

**Phase 6 — RPC and CLI.**
Extend `deltachat-jsonrpc` with the email methods (subject/recipients on send, threads, labels, raw source, policy). Harden `cli/` as the integration-test driver.

**Phase 7 — Desktop UI.**
Tauri v2 over the RPC surface. Message list, threaded reading pane, composer (subject, To/CC/BCC, attachments, quoting), label sidebar, search, settings, account setup, contacts with verification badges, QR display and camera scan. HTML mail rendered sandboxed with remote content blocked by default.

**Phase 8 — Later features.**
STUN-based direct device sync (reusing `calls.rs` ICE infrastructure), WebRTC audio/video calls, encrypted cloud backup (`imex/` with a cloud destination — important precisely because the mail server is not our storage), and optional at-rest database encryption. Delta Chat does not encrypt its local store at rest; we match that initially and treat it as a later opt-in feature.

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
