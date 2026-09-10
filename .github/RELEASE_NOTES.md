# eeemail v0.5.0 — the one where blocking blocks

An end-to-end-encrypted email client with classic email functionality, built on
a fork of [`chatmail/core`](https://github.com/chatmail/core).

**Blocking a sender did not stop their mail.** The engine set a flag and then
threw the answer away, so a blocked sender's mail was fetched, decrypted,
stored and marked read. This release makes blocking mean what it says, and adds
six more things: several accounts in one window, an email signature, message
importance that survives the wire, a searchable address book, and user-created
tags with colours.

## What is fixed

**A blocked sender's mail now stops at the door.** `Contact::block` set
`contacts.blocked`, and `receive_imf` computed `from_id_blocked` and then
discarded it. For a messenger that is deliberate — it is how contact requests
work. For a mail client it is a promise the user can watch being broken, in
their own inbox. `email::blocklist` rejects on arrival, and `block_sender`
writes both halves together so no caller can do one and not the other.

The undo path had the same shape in reverse: removing a block deleted the
pattern and left `contacts.blocked` set, hiding the contact from search while
their mail started arriving again. That one was found by re-reading the diff,
not by a failing test.

## What is new

**Several accounts in one window.** The engine has had `select_account` since
before this client existed; the frontend set the account once, at boot, and
never again. Switching is now in the sidebar.

**An email signature**, appended as text and as HTML, before `</body>` when the
body has one.

**Message importance** — high, normal or low — that travels as a real header,
so other clients see it and it survives a round trip.

**A searchable address book** with records and categories. `get_contacts` has
always taken a server-side substring filter; it was hardcoded `null`.

**Tags with colours**, created by you. Labels have carried a colour since
migration 166 and the client called exactly one of the eight label methods.

Three of the seven needed no engine work at all — the machinery was already
there and unreached. That is worth knowing before designing the next one.

## Known gaps

**Nobody has run this build.** The same gap v0.3.1 and v0.4.0 shipped with. The
blocklist at the centre of this release has not been exercised by a human in a
real application, and under `xvfb` in the preparation environment the webview's
web process does not start, so what the app *draws* was again not observed.

**The three live passes were not re-run**, and no CI job runs them. That matters
more than usual here: `send_email` gained a *seventh* parameter, and yerpc
compares positional arity with `!=`, so a six-argument call to it is `invalid
params` rather than a defaulted `None`. Eight call sites across the three
scripts were swept by reading, not by executing — the identical situation to
v0.4.0's sixth parameter, one release later.

**The blocklist matching rule has two implementations.** SQL for the hot path,
Rust for removal. `test_the_query_and_the_predicate_agree` pins them together,
but two expressions of one rule is the shape that already drifted once here.

**Importance is trusted as it arrives.** Any sender can mark their own mail
high, exactly as with any mail client. It is a display hint, not a claim.

**`%APPDATA%` is still the Windows profile**, and attachment filenames are still
only reduced to a last path component — both unchanged from v0.4.0, both still
wanting a migration and a stricter filter respectively.

Still unaudited, still prerelease. Use a dedicated mail account.

## Verification

Green on the tagged commit:

```
cargo nextest run --workspace                     1429 passed, 0 failed, 1 skipped
  ... --all-features                              1439 passed, 0 failed, 1 skipped
cargo test --workspace --locked --doc             0 failed
cargo clippy --workspace --all-targets            clean, both configs, -Dwarnings
cargo fmt --all -- --check                        clean
./scripts/check-fork-patches.sh                   clean
cd desktop && npm run check && npm run build      clean
./scripts/screenshots.sh                          13 images
```

CI is green on this commit across all seven jobs, Windows and the test mail
server included.

**Why nothing caught the thing that got through.** The feature branch merged
with its lint job red, putting four clippy errors on `main` for forty minutes.
CI reported two of them; there were four, because `--all-targets` stops at the
first crate that fails and never reached the other two. A CI log says why the
build stopped, not what is broken.

Two of the thirteen screenshots are not byte-stable across days: the trash
notice renders "still here for N more days" from the wall clock rather than the
pinned fixture clock. Known, unfixed, and recorded — it means a diff in those
two images is not necessarily a change in the UI.

**Run both nextest configurations.** `--all-features` carries ten tests the
default build does not, so a green default run is not a green CI.

## Installing

Download the installer for your platform, verify the `.sha256` beside it, and
run it. Full instructions in [`docs/INSTALL.md`](../docs/INSTALL.md).

```sh
sha256sum -c eeemail_0.5.0_amd64.deb.sha256
sudo apt install ./eeemail_0.5.0_amd64.deb
```

Or unzip `eeemail-windows-amd64.zip` / `eeemail-linux-amd64.zip` and run the app
from where it lands — on Windows, `eeemail.cmd` the first time.
[`docs/PORTABLE.md`](../docs/PORTABLE.md) is the guide, and it ships inside the
archive.

Licensed under MPL-2.0.
