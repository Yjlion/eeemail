# Architecture Decision Records

Each ADR records one decision, the context that forced it, and the consequences
we accepted. They are immutable once accepted: to change a decision, add a new
ADR that supersedes the old one rather than editing history.

Full design: [`../DESIGN.md`](../DESIGN.md) ·
Build & fork workflow: [`../development.md`](../development.md) ·
Testing: [`../testing.md`](../testing.md)

| # | Decision | Status |
|---|---|---|
| [0001](0001-fork-chatmail-core.md) | Fork `chatmail/core` rather than write a new engine | Accepted |
| [0002](0002-license-mpl-2.0.md) | License the project under MPL-2.0 | Accepted |
| [0003](0003-imap-as-transport.md) | IMAP/SMTP are transport only; single folder, no folder tree | Accepted |
| [0004](0004-local-store-and-raw-mime.md) | The local database is the mailbox; raw MIME retention is configurable | Accepted |
| [0005](0005-labels-not-folders.md) | Organize with labels/tags + archive, not a folder tree | Accepted · amended by [0009](0009-labels-and-search.md), [0017](0017-system-tags.md) |
| [0006](0006-encryption-policy.md) | Opportunistic encryption by default, with strict and lenient modes | Accepted |
| [0007](0007-server-template.md) | Ship a chatmail-derived Postfix/Dovecot template with traditional accounts | Accepted · `deploy/` deferred, see below |
| [0008](0008-email-message-model.md) | Recipient sets and threading are per-message; threading never merges by subject | Accepted |
| [0009](0009-labels-and-search.md) | Archive is the presence of a label; extended search is a separate entry point | Accepted |
| [0010](0010-server-retention.md) | Server retention is applied on arrival and is never retroactive | Accepted |
| [0011](0011-receipts-and-ephemeral.md) | Read receipts get a verified-only middle setting; ephemeral ships off | Accepted · ephemeral half superseded by [0019](0019-recoverable-ephemeral-expiry.md) |
| [0012](0012-rpc-and-cli.md) | eeemail defaults are applied at setup, not as compile-time defaults | Accepted |
| [0013](0013-desktop-ui.md) | The desktop shell is a JSON-RPC pipe; message content never runs in the app document | Accepted |
| [0014](0014-recipient-sets-on-the-wire.md) | A message carries its own recipients; Cc/Bcc use the same key path as members | Accepted |
| [0015](0015-at-rest-and-backup.md) | Database encryption is reported as partial, because the blobdir holds cleartext mail | Accepted · completed by [0020](0020-blobdir-encryption.md) |
| [0016](0016-structured-email.md) | Structured email is parsed for everyone and acted on only for trusted senders | Accepted |
| [0017](0017-system-tags.md) | System tags: derived where possible, stored where they must be | Accepted |
| [0018](0018-contact-gating.md) | Mail from strangers is held, not delivered, and expires if never accepted | Accepted |
| [0019](0019-recoverable-ephemeral-expiry.md) | Ephemeral expiry moves a message to Trash for 30 days instead of destroying it | Accepted |
| [0020](0020-blobdir-encryption.md) | The blobdir is encrypted with the database key, opt-in and off by default | Accepted |

## Deferred

**`server/deploy/` is deliberately not built** (2026-09-01). [0007](0007-server-template.md)
describes a full deployment — OpenDKIM, acmetool, MTA-STS, DNS record generation
and `filtermail` at the perimeter. `server/compose/` covers what the project
actually needs right now: a disposable target to develop and test application
functionality against. The deployment pieces all need a real domain and real
DNS, so they cannot be written honestly, let alone verified, from here. The
decision is to wait until there is a domain to deploy to rather than ship
configuration nobody has run.

**An Autocrypt header makes a key-contact** (2026-09-02).
[0021](0021-autocrypt-key-contacts.md) reverses, for eeemail, upstream `v2.59`'s
removal of Autocrypt-derived keys. The first live pass showed
[0006](0006-encryption-policy.md)'s opportunistic default was unreachable:
encryption follows contact type, and only a signed message or SecureJoin creates
the right kind of contact, so two correspondents who never scan a QR code can
never bootstrap. The key learned this way is unauthenticated and never counts as
verified.
