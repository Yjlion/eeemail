# Handoff — Phases 10–14b, and the first live pass

**Written 2026-09-01, updated 2026-09-03.** Branch `phase-0-foundation`.

## Where the project is

The engine is complete through Phase 14, the desktop client reads and writes,
and as of 2026-09-02 the whole thing has been **run end to end against a real
Postfix/Dovecot server** for the first time. What eeemail set out to be — a real
email client over Delta Chat's encryption — exists and demonstrably works. As
of 2026-09-02 it has also been run against **Delta Chat's own engine**, and as
of 2026-09-03 against **GnuPG**, so it is no longer only tested against itself
and its outgoing crypto is no longer only read by the library that wrote it.
Still unaudited, and still untested against Thunderbird, Gmail or any
mainstream provider.

```
cargo nextest run --workspace           1362 passed, 0 failed, 1 skipped
cargo clippy --all-targets -D warnings  clean
cargo fmt --check                       clean
scripts/check-fork-patches.sh           clean
cd desktop && npx tsc --noEmit          clean
./scripts/screenshots.sh                9 images, byte-stable across runs
python3 scripts/e2e-pass.py             all six steps pass
python3 scripts/interop-pass.py         all steps pass, against upstream v2.59.0
python3 scripts/gpg-interop-pass.py     all steps pass, against GnuPG 2.4.9
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

## The interop pass

[`scripts/interop-pass.py`](../scripts/interop-pass.py) runs eeemail against
**upstream's released `deltachat-rpc-server`**, pinned in
[`interop-upstream`](interop-upstream) and hash-checked. That binary is not a
stand-in for Delta Chat: the same release publishes the
`deltachat-stdio-rpc-server` tarball Delta Chat Desktop installs, so driving it
is driving Delta Chat's engine. What stays untested against a Delta Chat client
is its UI.

It shares `scripts/dcrpc.py` with the e2e pass — the wire client only, because a
framing bug fixed in one copy and not the other gives a green run that tests
nothing. Steps and account tables stay in each script; the two use different
mailboxes on purpose (`dana`/`erin` and `frank`/`grace`), since `alice`/`bob`
carry a completed SecureJoin from every e2e run, which would make the bootstrap
the pass exists to check unobservable.

### What it found

**1. A stock client will not touch cleartext, in either direction.** Upstream
defaults `force_encryption` on, and it is not advisory: it refuses to send an
unencrypted message (`chat.rs:2958`), refuses to *download* one
(`imap.rs:1694`), and trashes it if it arrives anyway (`receive_imf.rs:509`).
So ADR 0021's bootstrap can never begin with a shipped-default Delta Chat — the
first message is dropped before it is parsed and no Autocrypt header is ever
seen. The pass asserts that default, then turns it off, which is the
configuration Delta Chat offers for talking to ordinary email and the only one
in which classic mail flows at all. Everything after that single setting is what
the pass proves.

**2. ADR 0021 works, and it is the only thing that does.** Against a real
upstream engine: our first message is cleartext and it agrees; *its* reply is
cleartext too, because it imported our key and attached it to no contact; we
adopt its key, encrypt, and it decrypts and verifies; our signature then mints a
key-contact on its side and its next reply comes back encrypted — with nobody
having scanned anything. Step 2b is the tripwire for upstream reinstating
Autocrypt-derived contacts: if it ever passes with an encrypted reply,
`email::autocrypt::adopt` should be deleted rather than left to race it.

**3. Step 1b is why any of the rest means anything.** It calls
`apply_eeemail_defaults` on the stock account and requires JSON-RPC `-32601`.
Point both ends at our own binary and every other step still passes — the
failure mode this whole script exists to rule out. That negative has been
observed, not assumed.

**4. A held message is never released to a contact verified on another row.**
Found while writing step 5; issue #13, **fixed 2026-09-03**. `gating::release`
selected held mail with `WHERE m.from_id=?`, per contact row, while
`is_trusted` decided per person across rows. Cold mail is held on the sender's
*address* row — no signature meant no fingerprint at `receive_imf.rs:588` —
while SecureJoin verifies their *key* row and calls `release([key_contact])`,
which found nothing. `is_trusted` on the address row returned true by then
(`SecurejoinInvited` clears `is_known()`), so the mail was trusted and still
held until `purge` destroyed it at 30 days. This was finding #4 of the live
pass one row over.

The row-resolving query is now a shared `gating::same_person` that both call, so
they cannot drift apart again — that drift *was* the bug. Guarded by
`gating_tests::test_verifying_a_stranger_releases_the_mail_they_sent_cold`,
which fails on the old code, and by interop step 5, which now asserts the
release rather than only the hold.

**5. Threading onto a reply means onto what it replied to.** Adopting a key
moves the correspondence to the key-contact and so to a second chat. A stock
client replying there threads onto that message, not onto the cleartext
original — which is correct, and cost one wrong assertion to see.

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

- **Interop is done against Delta Chat's engine and GnuPG, and nothing else**
  (issue #5).
  `scripts/interop-pass.py` runs eeemail against upstream's released
  `deltachat-rpc-server` -- the same binary Delta Chat Desktop ships -- so
  Autocrypt, SecureJoin in both directions, and outbound classic email are now
  proven across an implementation boundary. The reason that mattered still
  holds for everything it does not cover: `e2e-pass.py` step 3b runs the same
  core on both sides and so proves nothing about interop.
  `scripts/gpg-interop-pass.py` (issue #14) adds the second OpenPGP
  implementation: GnuPG decrypts our PGP/MIME and verifies our signature, so
  our outgoing crypto has now been read by something that is not rPGP.
  **Thunderbird, Gmail and any mainstream provider remain untested**, and are
  not automatable in this environment.
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

1. Merge from upstream. The fork is still at `v2.59.0`; the longer that waits,
   the worse the first merge is — and ADR 0021 diverges from upstream on
   something upstream changed deliberately, so read that ledger note first.
   This is now first because `scripts/interop-pass.py` exists: until it did,
   there was no way to tell whether a merge had broken interop.
2. Add `scripts/e2e-pass.py` to CI beside the existing `mail-server` job, then
   the interop pass. Constraints already established, so they need not be
   rediscovered: the `mail-server` job has a 15-minute timeout, no Rust
   toolchain and no cache, while a cold `cargo build -p deltachat-rpc-server`
   is 10–20 minutes on its own — so this needs a job with
   `Swatinem/rust-cache` (`workspaces: core`), not that one. Bring the server
   up with `docker compose`, **not** the bare `docker run` the job uses today:
   that passes no `-e ACCOUNTS` and so provisions only alice and bob. The
   strict profile rewrites the outer Subject and conflicts with the e2e pass's
   subject assertion, so both passes belong on the permissive container. Cache
   the upstream binary on `docs/interop-upstream`, or build it from the
   vendored history, so the job does not depend on GitHub releases being up.
3. Thunderbird and a mainstream provider (#5) — the half of interop that is
   left, and the half a script in this environment cannot reach. #14 is done:
   `scripts/gpg-interop-pass.py` proves our outgoing PGP/MIME and signatures are
   readable by GnuPG. Thunderbird uses RNP rather than GnuPG, so that narrows
   the gap rather than closing it, and a real provider still needs credentials
   CI does not have.
4. Sending structured data, and a shell-mediated way to open a link — the two
   things Phase 14b deliberately left out.
