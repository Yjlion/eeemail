# 0001 — Fork `chatmail/core` rather than write a new engine

**Status:** Accepted — 2026-08-31

## Context

eeemail needs Autocrypt, SecureJoin, OpenPGP/PGP-MIME, ephemeral messages, MDN
read receipts, multi-device sync and encrypted backup. Delta Chat's engine
(`chatmail/core`, formerly `deltachat-core-rust`) implements all of them, in
Rust, interop-tested in production across Android, iOS and desktop.

An earlier draft of the design recommended writing a new engine instead. The
argument was that current `chatmail/core` had stripped its classic-email
accommodations: the `Config` enum retains only `ConfiguredInboxFolder`, having
dropped `MvboxMove`, `SentboxWatch`, `ShowEmails`, `SaveMimeHeaders`,
`DeleteServerAfter` and `FetchExistingMsgs`. Core watches one folder, exposes no
folder model, and retains no raw MIME.

That argument does not apply to this project. Single-folder IMAP-as-transport is
exactly the model we want (see [0003](0003-imap-as-transport.md)), so the
removals are alignment, not divergence. The genuine gap is only the
email-client layer.

## Decision

Fork `chatmail/core` into `core/`. Track upstream as a git remote and merge
periodically. Build the email-client layer on top.

## Consequences

**We gain**, working on day one: Autocrypt with gossip and Setup Message,
SecureJoin QR verification, PGP/MIME with RFC 9788 protected headers, IMAP sync
with IDLE and per-transport UID tracking, SMTP queueing, ephemeral timers, MDN,
`BccSelf` device sync, `imex` encrypted backup, WebRTC calls with STUN/TURN/ICE,
the provider autoconfig database, and a JSON-RPC surface.

**We accept:**

- A chat-shaped data model we must adapt. Decoupling message recipients from
  chat membership is the deepest change we take on.
- Divergence cost from upstream. Mitigated by fork discipline: concentrate
  additions in `core/src/email/`, patch upstream files as narrowly as possible,
  and record every upstream-file patch in `docs/fork-patches.md` with rationale.
- Upstream is actively removing classic-email affordances. Some future merges
  will conflict with our layer. This is a known, accepted maintenance cost.

**We must resist** ripping out unwanted features early. Group machinery,
`webxdc.rs` and `peer_channels.rs` are woven through `receive_imf/` and `chat/`.
Aggressive deletion in Phase 0 means fighting the compiler instead of building.
Disable at the API layer behind cargo features first; delete incrementally once
our layer is stable.
