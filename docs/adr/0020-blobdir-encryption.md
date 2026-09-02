# 0020 — The blobdir is encrypted with the database key, opt-in and off by default

**Status:** Accepted — 2026-09-01 · Completes the gap recorded in [0015](0015-at-rest-and-backup.md)

## Context

[0015](0015-at-rest-and-backup.md) shipped database encryption and reported it
as partial, because the blobdir holds attachments **and the raw MIME of every
retained message** in cleartext beside an encrypted database. It named what
closing the gap would take — per-blob AEAD, nonce management, key derivation, a
migration for existing blobs — and left it as its own piece of work rather than
half-doing it.

Upstream will not do this: they deprecated database encryption in 2025-11 partly
*because* of the blobdir, and recommend full-disk encryption instead. That
recommendation is correct and is not always available — a shared machine, a
synced profile directory, a backup that copies the blobdir somewhere the disk
encryption does not follow.

Raw MIME retention is eeemail's own addition, so this gap is one we widened.

## Decision

**Encrypt every blob** — attachments, avatars and retained raw MIME alike — with
XChaCha20-Poly1305, under a random 32-byte key **stored in the database**.

Not derived from the database passphrase, which is what this ADR said before
implementation contradicted it. Core does not keep the passphrase after opening
the database: SQLCipher holds it inside the connection, and `Sql` retains only
*whether* one was used. Deriving a blob key from it would mean patching upstream
to hold a secret in memory for the whole session, in exchange for nothing. A key
sitting in a database that SQLCipher already encrypts has the same property by a
shorter route.

**Opt-in, and off by default.**

**Enabling migrates the blobs already on disk; disabling migrates them back.**
The migration is resumable.

`email::vault::protection` stops hardcoding `blobs_encrypted: false` and reports
what is actually true.

## Consequences

- **One secret, not two.** The passphrase decrypts the database, and the
  database holds the blob key, so unlocking one unlocks the other and there is
  no second thing to lose. Changing the passphrase re-encrypts the database and
  the key rides along, so **not one byte of the blobdir has to be rewritten**
  — which a derived key would have required.
- **Blob encryption without a database passphrase is refused**, not merely
  discouraged. Storing the key in a cleartext database would protect nothing
  while `protection()` reported that it did, which is exactly the false belief
  [0015](0015-at-rest-and-backup.md) declined to create. `enable()` returns an
  error naming the reason.
- **Off by default is deliberate.** It costs CPU on every attachment read and
  write, it breaks any workflow that reaches into the blobdir with other tools,
  and its value depends entirely on a threat model — a stolen unlocked laptop is
  not helped by it at all. A user who turns it on should be doing so because
  they decided something, which is also why `protection()` reports rather than
  advises.
- **Encrypting on enable, rather than new blobs only, is the whole point.** A
  mailbox where the sensitive history stays in cleartext and only tomorrow's
  mail is protected would let `protection()` claim `blobs_encrypted` while the
  interesting bytes sat on disk — the exact false belief
  [0015](0015-at-rest-and-backup.md) refused to create.
- The migration walks a directory that may hold years of mail, and it must be
  interruptible. Each blob is converted with a write-to-temp-then-rename, so a
  crash leaves every blob either wholly converted or wholly not, and a restart
  resumes rather than starting over or corrupting what it already did.
- **Dedup hashes plaintext.** Content addressing is what makes two accounts
  receiving the same message store one copy
  ([0004](0004-local-store-and-raw-mime.md)); hashing ciphertext with a random
  nonce would give every copy a different name and quietly double disk usage.
  The consequence is that a filename reveals that two blobs are identical, which
  is a metadata leak we accept in exchange for the property.
- A blob that fails to decrypt is reported as unavailable, not fatal. One
  corrupt attachment must not make a mailbox unopenable.
- **The format carries a magic prefix, and that is what made this affordable.**
  `EEEBLOB1` at the head of an encrypted blob lets the read path be transparent:
  a file without it is returned untouched. So every read site in core became an
  unconditional one-line redirection instead of a conditional that has to know
  the setting, a blobdir part-way through a migration reads correctly, and
  turning the feature off does not have to be atomic with respect to readers.
- **`protection()` measures the blobdir rather than reading the setting.** The
  setting says what was asked for; the files say what is true, and after an
  interrupted pass those differ. `blobs_encrypted` is therefore false while any
  cleartext remains, which is the conservative direction.
- **The passphrase still has to come from somewhere.** Today that is a prompt.
  OS keyring integration is deliberately deferred — it is a per-platform
  dependency with its own threat model — and the prompt sits behind an RPC
  boundary chosen so a keyring can be dropped in behind it without redesign.
- This does not make eeemail safe against a running attacker, an unlocked
  machine, or memory. It closes exactly one gap: bytes at rest on a device that
  is off.
