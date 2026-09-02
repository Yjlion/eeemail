# Handoff — Phases 10–14b, and the first live pass

**Written 2026-09-01, updated 2026-09-02.** Branch `phase-0-foundation`.

## Where the project is

The engine is complete through Phase 14, the desktop client reads and writes,
and as of 2026-09-02 the whole thing has been **run end to end against a real
Postfix/Dovecot server** for the first time. What eeemail set out to be — a real
email client over Delta Chat's encryption — exists and demonstrably works,
unaudited and still untested against any other mail client.

```
cargo nextest run --workspace           1358 passed, 0 failed, 1 skipped
cargo clippy --all-targets -D warnings  clean
cargo fmt --check                       clean
scripts/check-fork-patches.sh           clean
cd desktop && npx tsc --noEmit          clean
./scripts/screenshots.sh                9 images, byte-stable across runs
python3 scripts/e2e-pass.py             all six steps pass
```

Run the suite with `cargo nextest`, never `cargo test` — see
[`testing.md`](testing.md) for why.

## The live pass

[`scripts/e2e-pass.py`](../scripts/e2e-pass.py) drives `deltachat-rpc-server`
over JSON Lines against [`server/compose`](../server/compose). It exists because
`cli/` cannot do this: the CLI is one-shot with no daemon, so it never starts
core's IO loop and can neither send nor receive. `DESIGN.md` claimed otherwise
for nine phases; it now says what is true.

```sh
cd server/compose && docker compose up -d --build && python3 smoke-test.py
cd ../.. && cargo build -p deltachat-rpc-server
python3 scripts/e2e-pass.py
```

The six steps are in [`DESIGN.md`](DESIGN.md#verification), which is where
this document used to claim they were and where they now actually are.

### What it found

**1. eeemail's defaults never applied in the real client.** `setup.ts` called
`apply_eeemail_defaults` *after* `add_transport`, and `policy::apply_defaults`
early-returns on `is_configured()`. Every account set up through the GUI
therefore kept upstream's policy: gating off, expiry destructive, encryption
strict. The module's own header comment described exactly this failure mode.
Fixed by ordering the call first; step 1 of the pass now asserts the defaults
landed rather than that the call was made.

**2. Mail to a verified contact went out in cleartext.** `compose::send`
resolved every `To` address with `Contact::add_or_lookup`, which returns the
*address*-contact. `Chat::is_encrypted` keys off the contact row's fingerprint,
so the chat was unencrypted however many keys we held for that person —
including one the user had verified by QR. Fixed by preferring a key-contact
when one exists; guarded by
`compose_tests::test_a_verified_contact_gets_an_encrypted_chat` and step 3b.

**3. Opportunistic encryption could not bootstrap at all.** Upstream `v2.59`
decides encryption by contact *type*, and mints a key-contact only from a signed
message or SecureJoin; Autocrypt peerstates are gone. Since eeemail could not
send encrypted first, it could never send a signed one, so two correspondents
who never scanned a QR code exchanged cleartext forever and ADR 0006's default
was unreachable. Resolved by [ADR 0021](adr/0021-autocrypt-key-contacts.md):
`email::autocrypt::adopt` makes a key-contact from the advertised header. The
key is unauthenticated and never counts as verified.

**4. Gating held every first encrypted reply.** Fallout from the above, found by
re-running the pass. The same correspondent is two contact rows — an
address-contact from the mail you sent them, a key-contact from the encrypted
reply — and `gating::is_trusted` asked only about the row the message arrived
on. So replies to the user's own mail went to Holding. Trust is now decided per
person, across rows sharing an address; verification still is not.

**5. A test-ordering trap worth keeping.** Writing to someone makes them known,
which releases their held mail. Any test that replies before checking Holding
dismantles what it is checking. The pass is ordered accordingly, with a comment.

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
`email::policy::apply_defaults` — which **never touches a configured account**,
so every caller must run it *before* configuring. That ordering was wrong in the
GUI for two phases and nothing caught it until the live pass.

**4. The blob key lives in the database, not in the passphrase.** ADR 0020
originally said HKDF from the passphrase; core does not keep the passphrase
after opening. The consequence worth keeping: blob encryption **requires** an
encrypted database, and `enable()` refuses rather than storing a key in
cleartext.

## Phase 14b — structured email

[ADR 0016](adr/0016-structured-email.md) is implemented:
`core/src/email/structured.rs`, migration 170, extraction from the existing
best-effort hook in `receive_imf_inner` (no new patch site), and a
`get_structured_data` RPC the reading pane renders. Trusted data gets a card;
untrusted gets the same fields inert, behind a notice — neither carries a link
or a button, because the shell has no mediated way to open anything yet. The
ADR was amended with the pinned trust predicate and with that decision.

Two things worth knowing before touching it. Extraction re-walks the raw MIME
with `mailparse` rather than reading `MimeMessage::parts`, because core drops an
`application/*` part with no filename as a "Missing attachment" — the exact
shape an SML part has; the tests assert that warning so the day it stops being
logged is a failure rather than a silent change. And `decoded_data` is empty
unless something was decrypted, so `store` takes `imf_raw` too.

## Not built, deliberately

**`server/deploy/`.** OpenDKIM, ACME, MTA-STS, DNS. Needs a real domain.

**OS keyring.** The passphrase path was built so a keyring drops in behind the
`set_database_passphrase` RPC without redesign.

## Known gaps

- **No interop testing** against Thunderbird, Gmail or a real Delta Chat client
  (issue #5). SecureJoin in step 3b runs between two eeemail accounts, which is
  the same core on both sides and so proves nothing about interop. This is still
  the gap that matters most for a mail client.
- **Housekeeping cannot be triggered on demand.** `gating::purge` and
  `ephemeral::purge` run only every `HOUSEKEEPING_PERIOD`
  (`scheduler.rs:449-453`), so the 30-day purge deadlines are not exercised
  live. Divert-to-trash and restore are.
- **Issue #2** — upstream drops recipients whose key is missing from the
  envelope while leaving them in the header. eeemail records who, and does not
  change the behaviour.
- **Camera QR scanning is not wired up.** Paste and file are the working paths.
- **Attachments are one per message**, because core carries one file per
  message. The composer says so rather than hiding it.
- **Nothing has been audited.**

## Suggested next steps, in order

1. Interop against Thunderbird and one real Delta Chat client (#5). Now the
   most valuable thing left, and the only one the live pass cannot substitute
   for: SecureJoin in step 3b runs between two eeemail accounts, which is the
   same core on both sides.
2. Merge from upstream. The fork is still at `v2.59.0`; the longer that waits,
   the worse the first merge is — and ADR 0021 now diverges from upstream on
   something upstream changed deliberately, so read that ledger note first.
3. Add `scripts/e2e-pass.py` to CI beside the existing `mail-server` job.
4. Sending structured data, and a shell-mediated way to open a link — the two
   things Phase 14b deliberately left out.
