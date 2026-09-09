# Handoff — Phases 10–14b, the first live pass, and v0.3.0

**Written 2026-09-01, updated 2026-09-09.** Branch `main`; `v0.3.0` is tagged
and published, **and does not work** — see immediately below. The eight desktop
issues #19–#26 landed after that; what they taught is in
[what the eight desktop features taught](#what-the-eight-desktop-features-taught).
Seven more features landed on `seven-features` after *that*; what they taught is
in [what the seven features taught](#what-the-seven-features-taught).

## The seven features

Multiple accounts, an email signature, message importance, a searchable address
book with records and categories, a blocklist that actually rejects mail, and
user-created tags with colours. Four ADRs — [0026](adr/0026-several-accounts-in-one-window.md),
[0027](adr/0027-a-blocklist-that-trashes-on-arrival.md),
[0028](adr/0028-contacts-are-an-address-book.md),
[0029](adr/0029-importance-travels-on-the-wire.md) — and three migrations, 172
to 174.

**Three of the seven needed no engine work at all.** Labels have carried a
colour since migration 166 and `create_label` has always taken one; the client
called exactly one of the eight label methods. `Accounts::select_account` and
the rest have existed since before this client did; `main.ts` did
`state.accountId = ids[0]` once, at boot. `get_contacts`' third parameter is a
server-side substring filter that was hardcoded `null`. Before designing
anything here, it is worth checking whether the engine already does it —
roughly half of this work was UI for machinery that was already present.

## What the seven features taught

### `Contact::block` does not block mail, and the name says it does

Core sets `contacts.blocked`, and `receive_imf` computes `from_id_blocked` and
then **discards it**. A blocked sender's mail is fetched, decrypted, stored and
marked read. For a messenger that is the contact-request feature; for a mail
client it is a promise the user can watch being broken. `email::blocklist` is
what actually rejects, and `block_sender` writes both halves so no caller can
do one.

### The same trap, in the other direction, found by re-reading the diff

`blocklist::remove` deleted the pattern and left `contacts.blocked` set. The
contact stayed hidden from `get_contacts` and filtered out of search while
their mail started arriving again — a half-undone block, which is exactly what
`block_sender` exists to prevent. Nothing failed; the tests passed; it was
found by reading the diff before committing. **Pairs of writes that must move
together want one function, and the undo path is a pair too.**

### A matching rule with two implementations will eventually have two answers

`blocklist::matches` is SQL, for speed on every incoming message;
`pattern_covers` is Rust, for the removal path. They express one rule.
`test_the_query_and_the_predicate_agree` pins them together over the cases that
distinguish them, which is the same medicine `gating::same_person` needed after
the two copies of *that* rule drifted apart in production.

### `git add -A` swept two unfinished files into a commit

The chunk-2 commit briefly contained the chunk-3 module, which its message said
nothing about. Inert — the module was not yet in `mod.rs`, so nothing compiled
it — and still a commit that lied about its contents. Reset, restaged, recommitted.

### Asserting on `payload()` is asserting on quoted-printable

The RFC 3676 separator goes out as `--=20`, `class="x"` as `class=3D"x"`, and
long lines carry soft breaks. Three signature assertions failed against a
perfectly correct implementation. `signature_tests::readable` undoes the three
for substring checks and says in its own doc comment that it is not a decoder.

### A bare `TestContext` refuses cleartext in both directions

It carries upstream's strict `ForceEncryption`, not eeemail's opportunistic
default. A send test fails with `e2e encryption unavailable`; a receive test
never reaches the module under test, because `receive_imf` drops the message
with "Fetched unencrypted message, ignoring". Sending tests want
`EncryptionMode::set(.., Opportunistic)`, receiving tests want
`t.allow_unencrypted()`.

### `send_email` grew a seventh parameter, and nothing in CI would have caught it

`yerpc` compares positional arity with `!=`, so a six-argument call to the
seven-parameter method is `invalid params`, not a `None`. Eight call sites
across `scripts/e2e-pass.py`, `scripts/interop-pass.py` and
`scripts/gpg-interop-pass.py` — **none of which any CI job runs.** They were
swept by hand in the same change. The ledger and the method's doc comment both
say so now; the next signature change owes the same sweep.

### Reading a header did not need a `HeaderDef` variant

The plan was to add one to an upstream enum, which is a patch per header
forever. `merge_headers` already lowercases every header into a `HashMap` that
`get_header` reads by name, so a four-line `get_header_raw` reads any of them.
The patch shrank from an enum variant plus a `MimeMessage` field plus a merge
change to one method.

### The build cannot be run in the foreground here

`cargo nextest run --workspace` needs longer than the ten-minute per-command
cap, and a `timeout` above that is silently clamped rather than honoured. The
result is exit 137 with no output at all, which looks exactly like a crash and
is not one — it cost two wrong diagnoses (OOM, then reduced parallelism) before
the cause was checked rather than guessed. **Run cargo in the background.**

## v0.3.1 — what installing v0.3.0 found

The step this document listed as next ("Install v0.3.0 and launch it from a
menu") was done. **The application did not work.** It installed correctly, drew
its window, and put an error where the mailbox should have been:

```
Command plugin:event|listen not allowed by ACL
```

**There was no `desktop/src-tauri/capabilities/` directory.** Tauri 2 resolves
its ACL by globbing that directory at build time; with no matches the compiled
`gen/schemas/capabilities.json` is `{}`. The four commands the shell registers
in `generate_handler!` are not ACL-checked and worked fine — but `listen()` is
the *core event plugin*, and it is. `desktop/src/rpc.ts` attaches the
`rpc-message` stream in the `Rpc` constructor and `call()` awaits it before
every request, so nothing resolved: no accounts, no mail, no events, on Linux
and Windows alike. Not a Windows bug. **Every platform, every install.**

### Why nothing here could see it, which is the part worth keeping

Nothing automated in this repository exercises the real Tauri IPC path.

- `scripts/screenshots.sh` photographs the **browser demo build**
  (`VITE_EEEMAIL_DEMO=1`), which answers from `desktop/src/fixtures.ts` and
  never calls `invoke`. `client.ts` picks the demo client before `rpc.ts` is
  even imported.
- `scripts/e2e-pass.py` drives `deltachat-rpc-server` over stdio. **The app does
  not use that binary** — it embeds the engine in-process. ADR 0022 says so and
  the archive comment says so, and it is exactly why the pass proves nothing
  about the shell.
- Every job in `ci.yml` was `ubuntu-latest`. The `windows-latest` leg of
  `release.yml` compiles and packages and runs nothing.

1375 tests, three live passes and eleven byte-stable screenshots were green
against an application that could not open a mailbox. This is the second bug of
that exact shape — `accounts_dir()` was the first — and ADR 0022 had already
written down "no test could have caught it" without drawing the conclusion.

### What v0.3.1 changes

- **`desktop/src-tauri/capabilities/default.json`**, granting
  `core:event:default` to the `main` window. `tauri.conf.json` now spells out
  `"label": "main"` so the capability's target is written down rather than
  inferred. `gen/` stays gitignored; `capabilities/` is source.
- **CI and the release workflow assert the *generated* ACL** grants an event
  permission. Checking the source file would prove nothing — it was the compiled
  artefact that was empty.
- **A `windows` job in `ci.yml`.** Build-only on Windows for eight releases is
  the structural cause of both bugs.
- **The desktop shell has tests**, its first: eleven, where there were none.
  `data_dir()` was written with `#[cfg]`, so it could only ever be checked on
  the platform it was wrong about; the platform is now a parameter and the
  Windows rule is tested from Linux. One test found a real defect while being
  written — the "an empty `XDG_DATA_HOME` is not a path" rule sat in the wrapper
  that reads the real environment rather than in the function under test, so it
  was never exercised.

  **`test_the_frontend_is_allowed_to_hear_the_engine` is the one that matters.**
  It builds the real `generate_context!()` and asks the compiled
  `RuntimeAuthority` whether `plugin:event|listen` resolves for the `main`
  window — the same authority the running app consults, not a re-reading of the
  source file. Moving `capabilities/` aside and re-running it reproduces the
  shipped failure verbatim (`plugin:event|listen is refused for the `main`
  window`), which is how the diagnosis was confirmed rather than assumed. It
  runs on Linux and on the new Windows job, needs no display, and is cheap.
- **A failed launch on Windows says why.** `windows_subsystem = "windows"` means
  a release build has no console, so `main`'s error and every `eprintln!` went
  to a stderr that does not exist — which is how the `accounts_dir()` bug stayed
  invisible for eight releases. `report_fatal` now shows a `MessageBoxW` with
  the error chain.
- **The release archive holds the app**, not only the two tools, and runs
  unzipped with its profile in `data/` beside the executable. Reproducing this
  bug required an installation, because installing was the only way to run it.
  [ADR 0024](adr/0024-portable-archives.md); ADR 0022 carries a dated amendment.
  The Windows zip carries the WebView2 bootstrapper and an `eeemail.cmd` that
  installs it, since only the NSIS installer bootstrapped the runtime.
- **`CLAUDE.md`** at the root, which did not exist.

### What was actually verified, and what was not

**Verified here.** The ACL test above, failing on the v0.3.0 arrangement and
passing on this one. The portable rule end to end: the real binary, copied into
a scratch folder beside an `eeemail-portable` marker, launched under `xvfb-run`,
put its profile in `<folder>/data/accounts/` and wrote nothing to
`~/.local/share/eeemail`. Then the gate:

```
cargo nextest run --workspace --locked     1376 passed, 0 failed, 1 skipped
  ... --all-features                       1386 passed, 0 failed, 1 skipped
cargo test --workspace --locked --doc      0 failed
cargo clippy --workspace --all-targets     clean, both configs, -Dwarnings
cargo fmt --all -- --check                 clean
scripts/check-fork-patches.sh              clean
desktop: npm run check, npm run build      clean
scripts/screenshots.sh                     11 images, byte-stable
```

The three live passes were **not** re-run: they exercise the engine, and the
whole diff to `core/` in this release is two version numbers in `Cargo.lock`.
Docker is not available where this was prepared either.

**Not verified, and it is the same gap as last time.** The webview never
executed a line of JavaScript in this environment — WebKitGTK's web process does
not start under `xvfb-run` here, so probes on `rpc_send` and `first_run_pending`
stayed silent whether the ACL was granted or not. The app starts, opens its
accounts directory and stays up; what it draws was not observed. **Nobody has
seen v0.3.1 render a mailbox.** That check needs a human at a machine, it is
what found this bug, and it is item 0 below.

One small thing running it did find: on Linux the system webview keeps an HSTS
cache at `~/.local/share/eeemail/hsts-storage.sqlite` regardless of the portable
marker, because WebKitGTK derives that path from the program name. It holds no
mail. `PORTABLE.md` says so rather than claiming the folder is airtight.

### What this leaves open

**The real IPC path is still untested end to end.** The guards catch an empty ACL and not
the next thing. Running the shell after touching it is the only check there is:

```sh
cd desktop && EEEMAIL_ACCOUNTS_DIR=/tmp/eeemail npm run tauri dev
```

**`%APPDATA%` is the roaming profile.** A SQLite mailbox and the whole blobdir
in a roaming profile means a domain-joined machine tries to sync it at logon,
and copying a live SQLite file is a corruption vector. `%LOCALAPPDATA%` is
right; it needs a migration and so is not in a fix release. Deliberately left.

**`stage_attachment` reduces a filename with `Path::file_name()`**, which strips
`..\` correctly on Windows but lets `name:stream` create an NTFS alternate data
stream and lets `CON`, `NUL` and `COM1` through. Untested, unfixed, noted.

## v0.3.0 — the release you install

Everything before this was an engine and a client that worked if you extracted
an archive and remembered a path. v0.3.0 is the first version a person installs.

- **Installers.** `.deb` and `.AppImage` on Linux, NSIS on Windows, built by
  `tauri build` in `release.yml` rather than `cargo build`. The bundler had been
  configured and never invoked since Phase 7. The archive stays and becomes the
  *tools*: `eeemail-cli` and `deltachat-rpc-server`, neither of which the app
  uses. [ADR 0022](adr/0022-desktop-distribution.md).
- **A first-launch dialog**, before the account list is read and so before the
  setup form asks for a mail password. Unaudited software; use a dedicated
  account and why; it still interoperates with ordinary clients; back it up. A
  `PREVIEW` chip in the sidebar afterwards.
  [ADR 0023](adr/0023-first-launch-disclosure.md).
- **Windows was broken and nobody could have noticed.** `accounts_dir()` read
  `XDG_DATA_HOME` then `HOME` and errored if it found neither — the ordinary
  state of a Windows session — so the Windows binary the matrix had been
  building since Phase 7 exited on launch. There is no Windows runner in CI and
  the function reads the environment rather than anything a unit test builds.
  Now `data_dir()` branches per platform.
- **`Holding` → `Unverified`**, all the way down, with the label row renamed in
  place by migration 171 so no message loses its tag.
- **All three deadlines end in Trash.** `gating::purge` became `gating::sweep`
  and moves mail rather than destroying it; both windows are now settings
  (`UnverifiedTrashDays`, `TrashPurgeDays`).
- **[`INSTALL.md`](INSTALL.md)** is the end-user document the project did not
  have.

### Five traps this left behind

**1. Migration 171 renames data three ways, and each fails silently.** The
`Config` key rename (`ephemeral_trash_days` → `trash_purge_days`) is the sharp
one: `Config` is stored under its snake_case name, so without the carry-over an
upgraded account drops to the compile-time default of `0` — destroy immediately
— which is the one value a user of that setting would never have picked. Guarded
by `migrations_tests::test_unverified_rename_migration`.

**2. `UnverifiedTrashDays` reads `0` as *never sweep*, not *sweep now*.** This is
the opposite of `TrashPurgeDays`, where `0` means destroy immediately, and the
two sit next to each other in Settings. The asymmetry is deliberate — someone who
wants unverified mail gone at once turns gating off, which releases it to the
inbox where they can delete it — but it is exactly the kind of thing a later
change will "tidy up". Guarded by `gating_tests::test_a_zero_window_never_sweeps`.

**3. The npm and Rust Tauri versions must move together.** `tauri build`
refuses to run when the `tauri` crate and `@tauri-apps/api` differ in
major/minor; `cargo build` never checked, so they had silently drifted to 2.2.5
and 2.11.1. Both npm packages are now pinned to **exact** versions for that
reason -- a caret range is what let them drift. The Rust side is capped by
`rust-version = 1.89`, which CI gates on, so moving Tauri forward means moving
the MSRV first, deliberately and in its own change.

**4. The unverified deadline is computed at sweep time, not stored.**
`held_msgs.purge_at` was dropped in migration 171 on purpose: the deadline is
`held_at` plus the *current* setting, so changing the setting moves mail already
waiting. Re-introducing a stored deadline would be a second source of truth that
silently outvotes the setting. Guarded by
`gating_tests::test_shortening_the_window_moves_mail_already_waiting`.

**5. One screenshot is not actually deterministic, and the README claims they
all are.** `fixtures.ts` pins `NOW` to a constant precisely so screenshots do
not drift, but `views/reading.ts:91` reads the *wall clock*
(`Date.now()`) to render the trash notice's "still here for N more days". The
number therefore falls by one every real day, so `trash.png` and
`trash-swept.png` change on a run that changed no code — while the README says
"they regenerate identically and a change in the images is a change in the UI".
`screenshots.sh` and `e2e-pass.py` step 6 both only compare two runs *on the
same day*, so neither can see it. The fix is to thread the fixture clock through
in demo mode rather than to regenerate the images.

## Shipping v0.3.0

Tagged `v0.3.0` on `main` and published as a prerelease on 2026-09-04, with
`.deb`, `.AppImage`, an NSIS `.exe`, both tool archives and a `.sha256` beside
each. Every asset was downloaded *from the release page* and checked rather than
read off a build log: checksums verify, the `.deb` carries `usr/bin/eeemail`,
its desktop entry and icons at three sizes, the `.AppImage` extracts to a valid
ELF, and the `.exe` is a real NSIS installer.

The release path is `push` of a `v*` tag. `workflow_dispatch` builds and uploads
the same artefacts **without** publishing, which is the rehearsal, and it is
worth doing every time — it is how the `.AppImage` was first proved buildable.

### What shipping it found

**1. The Windows job downloads its bundler at build time, and that is a coin
flip.** The tag build failed after compiling Windows cleanly, on
`https://github.com/tauri-apps/binary-releases/.../nsis-3.zip: Connection
Failed`. Unpinned, unretried, third-party, on every Windows release. The
rehearsal forty minutes earlier had succeeded on the same source, so no amount
of rehearsing prevents it; re-running the failed job was enough.

**2. A Windows blip blocks a Linux release that already built.** `Publish
release` is `needs: build`, so one red matrix leg skips publication of a leg
that succeeded. `fail-fast: false` exists in the same file to make partial
failure survivable and the comment above it says "a partial release is easier to
complete than to reconstruct" — the publish gate quietly undoes that. Worth
either a retry on the bundle step or a publish that tolerates partial success.

**3. CI runs the test suite in two configurations and it is easy to verify only
one.** `cargo nextest run --workspace` was 1365 tests at v0.3.0; `--all-features`
was 1375. (v0.3.1 adds eleven desktop tests to each: 1376 and 1386.)
Verifying the default alone produced a green local run and a red CI, and the
release notes had the same single-line gap. Both numbers are now recorded.

**4. `test_cache_is_cleared_when_io_is_started` is flaky, upstream, and will be
seen again.** It fails on `Logged an unexpected warning: no such table: chats`.
The location loop's first tick warns against the empty database of a
pseudo-configured account and `TestContext` fails any unexpected warning, so
whether the test finishes first is timing. Does not reproduce locally in either
configuration; passed on re-run. Not ours — neither `location.rs` nor the test
is touched by this fork.

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

Green on the released tree, 2026-09-04, everything re-run:

```
cargo nextest run --workspace              1365 passed, 0 failed, 1 skipped
  ... --all-features                       1375 passed, 0 failed, 1 skipped
cargo test --workspace --locked --doc      0 failed
cargo clippy --workspace --all-targets     clean, both configs, -Dwarnings
cargo fmt --all -- --check                 clean
scripts/check-fork-patches.sh              clean
cd desktop && npm run check && npm run build   clean
./scripts/screenshots.sh                   11 images, byte-stable across runs
server/compose/smoke-test.py               all checks pass
python3 scripts/e2e-pass.py                all six steps pass
python3 scripts/interop-pass.py            all steps pass, against upstream v2.59.0
python3 scripts/gpg-interop-pass.py        all steps pass, against GnuPG 2.4.9
```

**Run both nextest configurations.** CI does, and `--all-features` carries ten
tests the default build does not; the default alone is a green run that does not
mean a green CI. See "What shipping it found" above.

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

The steps are in [`DESIGN.md`](DESIGN.md#verification), which is where
this document used to claim they were and where they now actually are. There
were six; the seven features added three more, so the list is the count.

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
on. So replies to the user's own mail were held. Trust is now decided per
person, across rows sharing an address; verification still is not.

**5. A test-ordering trap worth keeping.** Writing to someone makes them known,
which releases their held mail. Any test that replies before checking the
unverified view
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

## What the eight desktop features taught

Issues #19–#26: the right-click menu, Sent, the unverified verify path, view
source, export, refresh, trash, and HTML composing. Three things came out of it
that outlive the features.

### Navigating the sidebar never re-read the list

`views/sidebar.ts` set `state.view` and called `changed()`. `changed()` repaints
from `state.messageIds`, and nothing on that path refetched it. So clicking
**Sent** drew the Sent heading, the Sent `aria-current`, and the *previous
view's* rows. Every system tag and every user label had this. It shipped because
`scripts/screenshots.sh` navigates by hash, and hash navigation went through
`main.ts`'s `reload()` — so the screenshots were correct and the application was
not.

`reload()` now lives in `desktop/src/nav.ts` and is what every view switch calls.
The rule: **change `state.view`, call `reload()`, not `changed()`.**

This is the third bug of the shape the v0.3.0 section describes — a green check
photographing a path the app does not take. It is worth saying plainly that the
demo build is not a test of the app, only of the views.

### `start_io` was never called at boot

The only `start_io` in the client was in the setup form. So the scheduler ran in
exactly one session — the one that created the account — and every launch after
that had no IMAP loop: nothing fetched, nothing queued sent, no `IncomingMsg`
ever emitted. The comment in `main.ts` said "new mail arrives pushed, not
polled", which was true, and nothing was pushing.

It is now called on every boot. Safe unconditionally: `Context::start_io`
returns early on an unconfigured account and `SchedulerState::start` is a no-op
when already started.

### yerpc checks positional arity exactly, and nothing in CI notices

`yerpc_derive` generates `if params.len() != n_inputs { invalid_args_len }`. A
trailing `Option<T>` does **not** make a parameter skippable: a five-argument
call to a six-parameter method is an error, not a `None`.

So adding `html` to `send_email` was a breaking change to every caller,
including `scripts/e2e-pass.py`, `scripts/interop-pass.py` and
`scripts/gpg-interop-pass.py` — **none of which CI runs**, because they need
Docker and a built `deltachat-rpc-server`. A stale positional call there is
caught by nothing. Any future RPC signature change has to sweep those three
scripts by hand.

### A trashed message can have no purge deadline, and `purge` will never see it

Found while building "empty trash". `email::ephemeral::to_trash` writes both the
`Trash` label and a `trashed_msgs` row, but a `Trash` label replayed from
another device arrives through `labels::sync_set` and writes only the label —
the deadline is a local decision and is deliberately never synced. That message
sits in the trash the user is looking at, with no `purge_at`, forever.

`empty()` takes the **union** of `in_trash()` and `trashed_msgs` so the button
empties what the user can see. That covers the symptom; it does not fix the
gap. A message in that state is still invisible to housekeeping. Guarded by
`ephemeral_tests::test_emptying_covers_a_trashed_message_with_no_deadline`.

### Adding a contact does not release their held mail

`Contact::create` reaches `add_or_lookup`, which writes the new origin with a
direct `UPDATE` rather than through `ContactId::scaleup_origin` — and
`scaleup_origin` is the only place carrying the `email::gating::release` hook.
So creating a contact for a held sender makes them **trusted** by `is_trusted`
and leaves their mail **held and invisible**, until `sweep` bins it weeks later.

The client calls `release_held_contact` immediately after `create_contact`. That
was chosen over adding a third `release` call site in `core/src/contact.rs`,
because the RPC already re-checks trust itself and the client costs nothing on a
future merge. Anyone adding another "add this person" path must make the same
pair of calls.

### The recipient set is not who to reply to, and Reply shipped broken

`msg_recipients` stores what a message was *addressed to*, and on receive that
is the incoming `To:` and `Cc:` headers verbatim. So on anything the user
received, **`To` is the user's own address**. Reading a reply's addressee out of
it addresses the reply to yourself and leaves the sender off it entirely.

**That is what `reading.ts` did, and it is in `v0.3.0` and `v0.3.1`.** Reply and
Reply all have been addressed to the user themselves for as long as the buttons
have existed; the comment above the line said "Reply goes to whoever the message
came from", which is what it should have done and not what it did. Moving the
code into `actions.ts` for the context menu is what made it visible, not what
broke it.

Nothing could have caught it. The screenshots render the composer, so an empty
or self-addressed `To` photographs the same as a correct one, and none of the
three live passes composes a *reply* through the client -- `e2e-pass.py` calls
`send_email` directly with addresses it chose itself. This is the fourth bug of
the shape this document keeps describing: a green check photographing a path the
app does not take.

The sender comes from `fromId`; the recipient set is what reply-*all* adds to it,
minus self. Replying to your own sent mail is the one case where the stored `To`
is the right answer, because there the sender is you and what the user means is
another message to the same people.

The module header in `email/recipients.rs` says plainly what is stored and from
where. It is worth reading before using it for anything: the set is per-message
and *directional*, and the two directions do not mean the same thing.

Self must be dropped case-insensitively, and `Cc` deduped against `To`. A reply
that copies the user on their own reply, or names someone twice in two
spellings, is a thread turning into duplicates.

### Parsing a URL with a base turns "reject" into "silently rewrite"

`richtext.ts` filters link schemes so a `javascript:` href cannot leave the
composer. It did that with `new URL(href, "https://invalid.example")` -- a base
supplied only so a scheme-less href would not throw. It does not throw; it
*resolves*, so `example.com` became `https://invalid.example/example.com`: a
link to a domain we invented, in someone's mail, looking exactly like one the
user chose. The dangerous schemes were still blocked, so the guard looked
correct.

`safeHref` now parses with no base at all and `normalizeHref` supplies the
scheme where the user types the address. A whitelist should reject what it does
not recognise, never rewrite it. [ADR 0025](adr/0025-composed-html.md).

### An author `display` rule silently defeats the `hidden` attribute

`[hidden] { display: none }` is a *user-agent* rule, so any author rule setting
`display` on the element beats it -- specificity does not come into it, because
author styles outrank UA styles outright. `.toolbar { display: flex }` was one,
so the composer rendered its formatting toolbar with `hidden` set and the
toolbar appeared anyway, above a plain-text box its buttons could not affect.

`styles.css` now carries a global `[hidden] { display: none !important; }`. The
`!important` is the point: the fix has to survive the next `display` rule
somebody adds, or this returns the first time a hidden element gets a flex
layout. Of the six elements rendered with `hidden`, only `.toolbar` had a
`display` rule, so nothing else moved -- and `composer.png` was the only
screenshot that changed.

Worth noting how it was found: `screenshots.sh` had been *comparing hashes*
across runs and passing, because a wrongly visible toolbar is perfectly
deterministic. Byte-stability says the UI did not change, never that it is
right. Somebody has to look at the images.

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
- **Housekeeping cannot be triggered on demand.** `gating::sweep` and
  `ephemeral::purge` run only every `HOUSEKEEPING_PERIOD`
  (`scheduler.rs:449-453`), so the 30-day deadlines are not exercised live.
  Divert-to-trash and restore are. Unit tests cover all three deadlines with a
  shifted clock; what is untested live is the scheduling, not the logic.
- **Launching an installed build is still the only real test of the app**, and
  it is done by hand. This was called "the largest untested surface in v0.3.0"
  and it was: the first person to install it found the app could not open a
  mailbox at all. Structure is not launch, and every artefact being built and
  structurally verified means nothing about whether it runs. The checks, still
  needing a human at a machine:
    1. Install each artefact on a clean machine and launch it **from the
       applications menu** -- the launcher entry is the thing under test, so
       starting it from a terminal proves nothing. Dialog appears, dismiss, set
       up an account, **send and receive** -- that last part is what would have
       caught the ACL bug, and reading the account list would not have.
    2. Unzip the archive and run it, on both platforms. On Windows via
       `eeemail.cmd`. Confirm the profile is in `data/` beside the executable
       and that an installed copy still uses `%APPDATA%\eeemail` and does not
       see the portable one's mail.
    3. On Windows, confirm the account directory is under `%APPDATA%\eeemail`
       for the *installed* copy. This is the bug v0.3.0 existed to fix and it
       has still never been observed working.
- **Issue #2** — upstream drops recipients whose key is missing from the
  envelope while leaving them in the header. eeemail records who, and does not
  change the behaviour.
- **Camera QR scanning is not wired up.** Paste and file are the working paths.
- **Attachments are one per message**, because core carries one file per
  message. The composer says so rather than hiding it.
- **Nothing has been audited.**

## The issue tracker, swept 2026-09-03

Worth knowing before trusting an issue title: **three of eleven were stale** --
the work had landed and nobody closed them. Read the code before believing an
issue describes the present.

Closed: **#13** (held mail never released to a contact verified on another row
-- a real bug; `gating::same_person` is now shared by `is_trusted` and
`release`, which is what stops them drifting apart again), **#8** (subject
duplicated into the body, fixed behind `Config::SubjectInBody` with upstream's
behaviour as the compile-time default, so zero upstream tests changed), **#14**
(the GnuPG pass), **#2** and **#3** (decisions, recorded in ADRs 0006 and 0019
rather than left open).

Open, with the stale parts corrected in a comment: **#1** (blob encryption is
implemented and wired in; what is left is `imex` tarring blobs raw, untested),
**#10** (`get_message_rows` batches the list already; only the deliberate `LIKE`
cap and paging remain), **#4** (composer, setup, contacts and QR display all
exist -- only camera scanning is missing, so the title overstates the gap by
three features), **#5**, **#9**, and **#6** -- whose comment records the finding
that `webxdc = ["peer-channels"]` means the two cannot be gated independently,
making it a phase-sized job rather than the afternoon its row implies.

One bug was found that no issue covered: `Message::save_file` copied blobs
byte-for-byte, so "save attachment" wrote an `EEEBLOB1` container when blob
encryption was on. Fixed.

## Suggested next steps, in order

0. **Install v0.3.1 and launch it from a menu**, on Linux and on Windows, and
   unzip the archive and run it. Doing this to v0.3.0 is what found the empty
   ACL; the fix has been unit-tested and its compiled artefact checked, but no
   installed v0.3.1 has been launched by a human yet, and that is precisely the
   gap that produced this release. Also confirm the portable profile lands in
   `data/` beside the executable and that an installed copy still uses
   `%APPDATA%\eeemail`.
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
