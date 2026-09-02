# eeemail v0.2.0 — the client half, and the first live run

An end-to-end-encrypted email client with classic email functionality, built on
a fork of [`chatmail/core`](https://github.com/chatmail/core).

**v0.1.0 was an engine preview with no composer and no account setup. This
release closes that**, and — more importantly — is the first version that has
been run end to end against a real mail server rather than only against its own
test suite. That run found four bugs, three of which no unit test could have
caught, because each needs a live account, a second party, or both.

## What works

| Area | State |
|---|---|
| Raw MIME retention with configurable expiry | ✅ |
| Per-message To/Cc/Bcc recipient sets, Cc/Bcc on the wire | ✅ |
| Conversation threading over `References`/`In-Reply-To` | ✅ |
| Labels, tags, archive, search, device sync | ✅ |
| Encryption policy: strict / opportunistic / lenient, per-contact | ✅ |
| Server retention: delete-after-download / keep N days / never | ✅ |
| Read-receipt policy, per-contact overrides | ✅ |
| Encrypted backup with staleness tracking | ✅ |
| JSON-RPC API and headless CLI | ✅ |
| Desktop reading client (Tauri) | ✅ |
| **Composer, account setup, contacts and QR in the GUI** | ✅ new |
| **System tags, contact gating, recoverable ephemeral expiry** | ✅ new |
| **Blobdir encryption at rest** | ✅ new |
| **Structured email (SML / Schema.org)** | ✅ new |
| **Verified end to end against a live server** | ✅ new |

Inherited from `chatmail/core` and interop-tested upstream: Autocrypt, SecureJoin
QR verification, PGP/MIME, protected headers, multi-device sync.

## New in this release

**Encryption now actually happens.** Upstream `v2.59` decides encryption by
contact *type* and mints the right kind of contact only from a signed message or
a QR verification. eeemail could never send a first encrypted message, so it
could never send a signed one — two correspondents who never scanned each
other's code exchanged plaintext forever. eeemail now learns a correspondent's
key from the `Autocrypt:` header they advertise. See **Things to know** below,
because this is a real trade and not a free win.

**Mail to a verified contact is no longer sent in cleartext.** The composer
addressed the wrong contact record, so verifying someone by QR appeared to work
and bought you nothing. Fixed and covered by a regression test.

**Your settings now apply.** Account setup called for eeemail's defaults *after*
configuring the account, at which point the engine refuses them. Every account
created through the GUI silently ran on upstream's policy: gating off, expiry
destructive.

**Mail from strangers waits in Holding** for 30 days rather than reaching the
inbox, and a fired expiry timer moves a message to Trash where it stays readable
and restorable, rather than destroying it.

**Structured email.** A message that carries a machine-readable description of
itself — a parcel, a booking — is parsed for every sender and shown. Data from a
sender you have engaged with, over encrypted and signed mail, renders as a card;
everything else renders inert. Neither carries a link or a button.

## Things to know before trusting it

- **Keys learned from an `Autocrypt:` header are not authenticated.** Anyone who
  can write the `From` line of a first message can write the `Autocrypt` line.
  This protects you from someone reading stored mail; it does **not** protect you
  from someone rewriting mail in flight. That is Autocrypt's own threat model,
  and it is why "encrypted" and "verified" are two separate badges in the UI —
  only a QR verification survives an active attacker. This deliberately reverses
  an upstream decision; see
  [ADR 0021](../docs/adr/0021-autocrypt-key-contacts.md).
- **Nothing has been interop-tested.** Not against Thunderbird, not Gmail, not a
  real Delta Chat client. The SecureJoin exercise in our end-to-end run is
  between two eeemail accounts, which is the same code on both sides and proves
  nothing about talking to anyone else. This is the largest gap in the project.
- **Blob encryption is opt-in and needs a database passphrase.** Until you set
  one, attachments and retained message sources stay in cleartext in the
  blobdir. The app reports what is and is not protected rather than claiming
  otherwise.
- **Encrypted mail can silently omit a recipient.** Recipients whose key is
  missing are dropped from the envelope but left in the headers — upstream
  behaviour we surface rather than change. eeemail records who never received it.
- **Camera QR scanning is not wired up.** Paste and file are the working paths.
- **One attachment per message**, because core carries one file per message. The
  composer says so rather than hiding it.
- Most of this was written by a large language model under human direction. It is
  reviewed and tested; it has **not** been audited by a security professional,
  and an encrypted mail client is exactly the kind of software where that
  distinction matters. Do not rely on it for anything that matters yet.

## Verification

```
cargo nextest run --workspace           1358 passed, 0 failed, 1 skipped
cargo clippy --all-targets              clean
cargo fmt --check                       clean
scripts/check-fork-patches.sh           clean
desktop: tsc --noEmit, npm run build    clean
scripts/screenshots.sh                  9 images, byte-stable across runs
python3 scripts/e2e-pass.py             all six steps pass, live
```

## Installing

Download the archive for your platform, verify the `.sha256` beside it, and
extract. It contains `eeemail-cli`, `eeemail-desktop` and
`deltachat-rpc-server`.

Licensed under MPL-2.0.
