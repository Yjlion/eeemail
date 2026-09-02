# 0015 — Database encryption is reported as partial, because the blobdir holds cleartext mail

**Status:** Accepted — 2026-09-01 · The blobdir gap it records is closed by [0020](0020-blobdir-encryption.md)

## Context

[0004](0004-local-store-and-raw-mime.md) noted that core links `rusqlite` with
`bundled-sqlcipher-vendored-openssl` and applies `PRAGMA key` when a passphrase
is set, so at-rest encryption is a key-management problem rather than an engine
change.

Building it surfaced why upstream **deprecated** the feature in 2025-11:

> Db encryption does nothing with blobs, so fs/disk encryption is recommended.

For Delta Chat that leaves attachments in the clear. For eeemail it leaves
attachments **and the raw MIME of every retained message** — complete original
messages, headers, subjects and bodies — sitting in the blobdir next to an
encrypted database. Raw MIME retention is eeemail's own addition, so we made
this gap wider than upstream's.

Separately, because the server is a transport spool
([0003](0003-imap-as-transport.md)), the local database is the only durable copy
of the mailbox. A lost device with no backup is a lost mailbox, with nothing on
the server to re-download.

## Decision

**Ship database encryption, and report it as partial.**
`email::vault::protection` returns `blobs_encrypted` — always `false` — the
number of cleartext bytes in the blobdir, and a `summary` sentence naming both.

**Backup requires a passphrase**, where core's `imex` accepts an optional one.

**Record when a backup was taken and report staleness**; do not schedule one.

## Consequences

- A settings screen that renders `protection` faithfully cannot claim more than
  is true. Reporting only `databaseEncrypted` would tell a user their mail is
  protected when the most sensitive part of it is a `cat` away — worse than
  offering nothing, because it converts a known gap into a false belief.
- `blobs_encrypted` is a field rather than an omission on purpose: a caller
  cannot show "encrypted" without also having been handed the fact that half
  the data is not.
- Full at-rest protection needs the blobdir encrypted too — per-blob AEAD, nonce
  management, key derivation and a migration for existing blobs. That is real
  work and is tracked as its own issue rather than half-done here. Until it
  lands, filesystem or full-disk encryption is the honest recommendation, and
  the summary says so.
- An unencrypted backup is a copy of the entire mailbox in the clear. That must
  not be reachable by leaving a field blank, so the passphrase is required and
  an empty one is an error rather than a default.
- Staleness is reported, not acted on. An automatic backup needs a destination,
  and every such place is a decision with consequences the user has to make: a
  cloud provider sees the ciphertext, its size and its timing, and so learns
  when you use your mail.
- **Uploading to a specific cloud provider is deliberately not implemented.** It
  needs credentials, a provider API and a threat model per provider, none of
  which can be built or tested honestly without picking one. Exporting to a path
  is the provider-independent part, and it is what a synchronising folder or an
  external disk needs anyway.
- Never having backed up is reported as *stale*, not as a neutral state: it is
  precisely the case where losing the device costs everything.
