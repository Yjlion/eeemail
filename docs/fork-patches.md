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

- Remote: `https://github.com/chatmail/core`
- Forked at: _(record commit SHA when the fork lands in Phase 0)_
- Last merged: _(none yet)_

## Patches to upstream files

| File | Change | Why | ADR |
|---|---|---|---|
| _(none yet)_ | | | |

## New files (not upstream, no merge risk)

| File | Purpose | ADR |
|---|---|---|
| `core/src/email/rawmime.rs` | Raw MIME blob store + retention/expiry | [0004](adr/0004-local-store-and-raw-mime.md) |
| `core/src/email/recipients.rs` | To/CC/BCC sets decoupled from chat membership | [0001](adr/0001-fork-chatmail-core.md) |
| `core/src/email/threading.rs` | JWZ threading over References/In-Reply-To | [0001](adr/0001-fork-chatmail-core.md) |
| `core/src/email/labels.rs` | Labels/tags + archive | [0005](adr/0005-labels-not-folders.md) |
| `core/src/email/policy.rs` | Encryption strictness, retention, MDN/ephemeral defaults | [0006](adr/0006-encryption-policy.md) |
