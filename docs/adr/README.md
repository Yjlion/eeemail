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
| [0005](0005-labels-not-folders.md) | Organize with labels/tags + archive, not a folder tree | Accepted |
| [0006](0006-encryption-policy.md) | Opportunistic encryption by default, with strict and lenient modes | Accepted |
| [0007](0007-server-template.md) | Ship a chatmail-derived Postfix/Dovecot template with traditional accounts | Accepted |
| [0008](0008-email-message-model.md) | Recipient sets and threading are per-message; threading never merges by subject | Accepted |
| [0009](0009-labels-and-search.md) | Archive is the presence of a label; extended search is a separate entry point | Accepted |
| [0010](0010-server-retention.md) | Server retention is applied on arrival and is never retroactive | Accepted |
| [0011](0011-receipts-and-ephemeral.md) | Read receipts get a verified-only middle setting; ephemeral ships off | Accepted |
| [0012](0012-rpc-and-cli.md) | eeemail defaults are applied at setup, not as compile-time defaults | Accepted |
| [0013](0013-desktop-ui.md) | The desktop shell is a JSON-RPC pipe; message content never runs in the app document | Accepted |
