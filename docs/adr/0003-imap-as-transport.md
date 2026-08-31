# 0003 — IMAP/SMTP are transport only; single folder, no folder tree

**Status:** Accepted — 2026-08-31

## Context

A traditional email client treats the IMAP server as the mailbox: folders are
server-side, messages live there indefinitely, and the client is a view onto
remote state. That model is incompatible with the privacy properties we want.
If the server holds the mail, the server holds metadata about the mail, and
decrypted content cannot be stored without handing plaintext back to the server.

Building a new server stack instead is not attractive: IMAP and SMTP are
standard, well-tested server implementations already exist, and a user should be
able to reuse their existing mail platform.

## Decision

Use IMAP and SMTP purely as message transport. Watch a single IMAP folder.
Download messages, decrypt them, store them locally, and remove them from the
server. Never write decrypted content back to the server. Do not present a
folder tree.

This mirrors the chatmail relay model: Postfix accepts and relays, Dovecot
spools to Maildir until download, expiry and quota rules clear the remainder.

## Consequences

- The server becomes a spool with a short retention horizon. Its storage
  requirements are small and bounded.
- Server-side metadata exposure is minimized: mail that has been collected is
  gone.
- Server deletion is configurable — delete-after-download (default), keep N
  days, or never — the last being an opt-in coexistence mode for users who also
  run Thunderbird or webmail against the same account. Coexistence is not the
  default because leaving mail on the server weakens the threat model.
- **All organization becomes local** (see [0005](0005-labels-not-folders.md)),
  and **the local database becomes the only copy** of the mailbox (see
  [0004](0004-local-store-and-raw-mime.md)). Backup therefore stops being a
  nicety and becomes a correctness requirement.
- Adding a new device cannot be done by re-syncing from the server. It requires
  device-to-device transfer or a backup, both of which core already implements.
