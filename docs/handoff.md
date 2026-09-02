# Handoff — Phases 10–14a

**Written 2026-09-01.** Branch `phase-0-foundation`, pushed through
`81944da66`. Three commits: docs (`01711dc62`), engine + desktop
(`8eb44374d`), screenshots (`81944da66`).

## Where the project is

The engine is complete through Phase 13 and the desktop client can now read,
write, set itself up, manage contacts and verify by QR. What eeemail set out to
be — a real email client over Delta Chat's encryption — exists end to end,
unaudited and untested against any other mail client.

```
cargo nextest run --workspace           1337 passed, 0 failed, 1 skipped
cargo clippy --all-targets -D warnings  clean
cargo fmt --check                       clean
scripts/check-fork-patches.sh           clean
cd desktop && npx tsc --noEmit          clean
./scripts/screenshots.sh                8 images, byte-stable across runs
```

Run the suite with `cargo nextest`, never `cargo test` — see
[`testing.md`](testing.md) for why.

## What landed

**Phase 10 — docs.** README rewritten with a real status table and a plain
statement that most of this was written by an LLM and has not been audited.
Five ADRs: [0016](adr/0016-structured-email.md) structured email,
[0017](adr/0017-system-tags.md) system tags,
[0018](adr/0018-contact-gating.md) contact gating,
[0019](adr/0019-recoverable-ephemeral-expiry.md) recoverable expiry,
[0020](adr/0020-blobdir-encryption.md) blobdir encryption. Issue #7 closed.

**Phase 11 — system tags, gating, recoverable expiry.** Migration 169.
`email::tags`, `email::gating`, `email::ephemeral`.

**Phase 12 — desktop MVP.** `desktop/src/views/` — composer, setup, contacts,
settings, sidebar, list, reading. `email::compose::send`, `send_email`,
`get_message_rows`.

**Phase 13 — encryption at rest.** `email::blobcrypt`, and a
`vault::set_passphrase` that can now actually encrypt something.

**Phase 14a — screenshots.** `scripts/screenshots.sh`, `screenshots/`.

## The four things most likely to bite you

**1. `email::ephemeral::divert` must stay above `select_expired_messages`.**
It is the only patch in the ledger whose *position* is the patch. `divert` works
by clearing the column the select reads. A merge that keeps the call but moves
it below compiles, passes most tests, and silently restores the destructive
expiry that ADR 0019 exists to remove. Guarded by
`ephemeral_tests::test_expiry_clears_the_timer_so_core_does_not_destroy_it`.

**2. `blob.rs` encrypts *after* the hash.** Content addressing hashes plaintext,
because hashing ciphertext under a random nonce would give every copy of the
same message a different name and quietly double the blobdir. Guarded by
`blobcrypt_tests::test_dedup_still_hashes_plaintext`.

**3. eeemail's defaults are applied at setup, not compiled in.** `InboxGating`
and `EphemeralTrashDays` both ship as upstream's behaviour and are turned on by
`email::policy::apply_defaults`. Flipping the compile-time defaults broke eight
upstream tests. If you add another behavioural default, do it the same way —
and note that `apply_defaults` deliberately never touches a *configured*
account, so tests using `TestContext::new_alice()` must set values explicitly.

**4. The blob key lives in the database, not in the passphrase.** ADR 0020
originally said HKDF from the passphrase; core does not keep the passphrase
after opening, so that would have meant patching upstream to hold a secret in
memory all session. The ADR was amended to match the code. The consequence
worth keeping: blob encryption **requires** an encrypted database, and
`enable()` refuses rather than storing a key in cleartext.

## Not built, deliberately

**Structured email (ADR 0016).** Designed, not implemented — matching the
instruction to add it to the design and implement as we develop. When it lands:
`core/src/email/structured.rs`, migration 170, parse from the existing
best-effort hook in `receive_imf_inner` (no new patch site), store
`structured_data(msg_id, json, trusted)` with the trust verdict computed once at
receive. Trusted data may drive affordances; untrusted renders inert.

**`server/deploy/`.** OpenDKIM, ACME, MTA-STS, DNS. Needs a real domain. Issue
#7 closed with the reasoning.

**OS keyring.** The passphrase path was built so a keyring drops in behind the
`set_database_passphrase` RPC without redesign.

## Known gaps

- **Not verified end to end against `server/compose`.** Everything above is unit
  tests plus the frontend build. The six-step manual pass is in
  [`DESIGN.md`](DESIGN.md#verification): set up an account in the GUI, send a
  Cc'd message, watch held mail move to the inbox on verification, let a timer
  expire and restore it, enable blob encryption, regenerate screenshots.
- **No interop testing** against Thunderbird, Gmail or a real Delta Chat client
  (issue #5). This is the gap that matters most for a mail client and it needs a
  second live client.
- **Issue #2** — upstream drops recipients whose key is missing from the
  envelope while leaving them in the header. eeemail records who, and does not
  change the behaviour.
- **Camera QR scanning is not wired up.** The Linux webview does not reliably
  give a page a camera; paste and file are the working paths.
- **Attachments are one per message**, because core carries one file per
  message. The composer says so rather than hiding it.
- **Nothing has been audited.**

## Suggested next steps, in order

1. Bring up `server/compose` and run the six-step manual pass. It is the only
   thing standing between "tests pass" and "this works".
2. Interop against Thunderbird and one real Delta Chat client (#5). Autocrypt
   and SecureJoin are inherited, not tested by us.
3. Structured email (ADR 0016).
4. Merge from upstream. The fork is still at `v2.59.0` and the ledger now has
   eleven more patch sites than it did; the longer that waits, the worse the
   first merge is.
