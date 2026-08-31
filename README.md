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

**Design phase.** No implementation yet.

- [`docs/DESIGN.md`](docs/DESIGN.md) — full design and phased implementation plan
- [`docs/adr/`](docs/adr/) — architecture decision records

## License

[MPL-2.0](LICENSE). See [`NOTICE`](NOTICE) for bundled dependencies.
