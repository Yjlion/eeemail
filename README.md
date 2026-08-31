# eeemail

An end-to-end encrypted email client with classic email functionality.

Delta Chat solved the hard parts of usable email encryption — Autocrypt key
exchange, QR-code contact verification that resists active MITM attacks,
ephemeral messages, multi-device sync — but presents them as a chat client, and
upstream has no interest in a traditional email interface.

eeemail keeps the encryption engine and builds the email client: subjects,
CC/BCC, threads, attachments, search, drafts and labels.

## How it works

IMAP and SMTP are used as **transport only**, not storage. A single IMAP folder
is watched; messages are downloaded, decrypted, stored locally and removed from
the server. Decrypted content is never written back. The local database is the
mailbox.

Encryption is opportunistic by default — encrypted whenever the recipient's key
is known, cleartext otherwise, clearly marked either way — and can be set
stricter (E2E only) or more lenient.

## Status

**Phase 0 — foundation.** `core/` is a fork of
[`chatmail/core`](https://github.com/chatmail/core) at `v2.59.0`, vendored via
`git subtree`. No email-client code yet.

- [`docs/DESIGN.md`](docs/DESIGN.md) — full design and phased implementation plan
- [`docs/adr/`](docs/adr/) — architecture decision records
- [`docs/development.md`](docs/development.md) — build and fork workflow
- [`docs/testing.md`](docs/testing.md) — why the suite needs `cargo nextest`
- [`docs/out-of-scope.md`](docs/out-of-scope.md) — upstream features slated for removal

## License

[MPL-2.0](LICENSE). See [`NOTICE`](NOTICE) for bundled dependencies.
