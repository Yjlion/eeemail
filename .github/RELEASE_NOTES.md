# eeemail v0.1.0 — engine preview

An end-to-end-encrypted email client with classic email functionality, built on
a fork of [`chatmail/core`](https://github.com/chatmail/core).

**This is a preview of the engine, not a finished client.** The reading UI works;
there is no composer and no account setup in the GUI yet. Use `eeemail-cli` to
configure an account.

## What works

| Area | State |
|---|---|
| Raw MIME retention with configurable expiry | ✅ |
| Per-message To/Cc/Bcc recipient sets | ✅ |
| Conversation threading over `References`/`In-Reply-To` | ✅ |
| Labels, archive, device sync | ✅ |
| Search over body, subject, recipients, labels | ✅ |
| Encryption policy: strict / opportunistic / lenient, per-contact | ✅ |
| Server retention: delete-after-download / keep N days / never | ✅ |
| Read-receipt policy, per-contact overrides | ✅ |
| Cc/Bcc on the wire | ✅ |
| Encrypted backup with staleness tracking | ✅ |
| JSON-RPC API and headless CLI | ✅ |
| Desktop reading client (Tauri) | ✅ |
| Composer, account setup, contacts, QR in the GUI | ❌ not yet |
| Ephemeral messages | machinery complete, **ships disabled** |
| Blobdir encryption at rest | ❌ not yet — see below |

Inherited from `chatmail/core` and interop-tested upstream: Autocrypt, SecureJoin
QR verification, PGP/MIME, protected headers, multi-device sync, WebRTC calls.

## Things to know before trusting it

- **Database encryption is partial.** Encrypting the SQLite database leaves
  attachments and the original source of every retained message in cleartext in
  the blobdir. The app reports this rather than claiming otherwise. Use
  filesystem or full-disk encryption. See
  [ADR 0015](../docs/adr/0015-at-rest-and-backup.md).
- **Ephemeral messages ship disabled** even though the plan called for them on by
  default: ephemeral deletion removes the local copy, and the local database is
  the only durable copy of the mailbox. See
  [ADR 0011](../docs/adr/0011-receipts-and-ephemeral.md).
- **Encrypted mail can silently omit a recipient.** When a message goes out
  encrypted, recipients whose key is missing are dropped from the envelope but
  left in the headers — upstream behaviour we surface rather than change. eeemail
  records them and can tell you who never received it.
- The desktop app has no account setup. Configure with `eeemail-cli` first.
- Not audited. Do not rely on it for anything that matters yet.

## Installing

Download the archive for your platform, verify the `.sha256` beside it, and
extract. It contains `eeemail-cli`, `eeemail-desktop` and
`deltachat-rpc-server`.

Licensed under MPL-2.0.
