# 0004 — The local database is the mailbox; raw MIME retention is configurable

**Status:** Accepted — 2026-08-31

## Context

Because the server is a transport spool ([0003](0003-imap-as-transport.md)), the
local store is the only durable copy of a user's mail.

Two things need storing, and they pull in opposite directions:

- **Decrypted content** must be stored. Keys can be lost, and a mailbox that
  becomes unreadable when a key is lost is not a mailbox. Delta Chat's core
  already stores decrypted, parsed content.
- **Raw MIME** is what makes view-source, signature re-verification, faithful
  reply and forward quoting, and standards-conformant export possible. Core does
  **not** store it: `SaveMimeHeaders` was removed and `message.rs` retains only
  `rfc724_mid` and `mime_in_reply_to`.

Storing both, indefinitely, roughly doubles disk usage for no benefit most of
the time — the raw form matters mainly while a message is in flight and during
the window in which it may be replied to or forwarded.

## Decision

Always store decrypted content; it is canonical and never expires by default.

Store raw MIME in a content-addressed blob store with a **configurable
retention period**, short by default (covering transport and reply time), which
the user may extend to indefinite.

## Consequences

- `raw_mime(msg_id, blob_hash, size, received_at, expires_at)`, with a NULL
  `expires_at` meaning keep forever. Expiry runs in core's existing housekeeping
  job.
- Features that need the raw form must degrade gracefully once it has expired.
  View-source and signature re-verification become unavailable for old messages;
  the UI must say so rather than fail. Reply and forward quoting must work from
  decrypted content alone.
- Users who need archival fidelity — or who may need to prove what they
  received — can set retention to indefinite and accept the disk cost.
- At-rest database encryption is deliberately **not** in the initial scope.
  Delta Chat does not encrypt its local store either. It is a later opt-in
  feature, tracked in Phase 8 alongside encrypted cloud backup.
