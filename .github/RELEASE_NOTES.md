# eeemail v0.4.0 — the one where Reply goes to the sender

An end-to-end-encrypted email client with classic email functionality, built on
a fork of [`chatmail/core`](https://github.com/chatmail/core).

**Reply was addressed to you.** In v0.3.0 and v0.3.1, replying to a message
put *your own address* in the `To` field and left the sender off it entirely.
This release fixes that, and adds the eight things the client was missing to be
usable for ordinary mail: a right-click menu, Sent, view source, export,
refresh, empty trash, a verify path for unverified senders, and formatted
composing.

## What is fixed

**Reply and Reply all now address the sender.** The reply's `To` was read from
the message's stored recipient set — but on receive, `msg_recipients` holds the
incoming `To:` and `Cc:` headers verbatim, so on anything you received that
field *is your own address*. The comment above the line said "Reply goes to
whoever the message came from", which is what it should have done and not what
it did. The sender now comes from the message's `from_id`; Reply all adds the
other addressees, minus you, case-insensitively, with `Cc` deduplicated against
`To`. Replying to your own sent mail still uses the stored `To`, which is the
one case where it is the right answer.

**Why nothing caught it, which is the more useful half.** The screenshots
photograph a rendered composer, and a self-addressed `To` photographs exactly
like a correct one. None of the three live passes composes a *reply* through the
client — they call `send_email` directly with addresses they chose themselves.
This is the fourth defect of that shape in this project's short history, and
`docs/handoff.md` now lists it beside the other three.

**Navigating the sidebar never re-read the message list.** Clicking Sent set the
view and repainted from a list nothing had refetched, so you got the Sent
heading over the previous view's rows. Every system tag and every user label had
this. It survived because `scripts/screenshots.sh` navigates by URL hash, and
hash navigation went down a different code path — so the screenshots were
correct while the application was not.

**The IMAP loop only ran in the session that created the account.** `start_io`
was called in the setup form and nowhere else, so every launch after the first
fetched nothing, queued nothing, and emitted no new-mail event. It is now called
at every boot.

**A trashed message could have no expiry deadline, and be invisible forever.**
Moving mail to the trash writes both a `Trash` label and a deadline row, but a
`Trash` label replayed from another device writes only the label — the deadline
is a local decision and is deliberately never synced. Such a message sat in the
trash you were looking at, and housekeeping could never see it. Emptying the
trash now takes the union of both, so the button empties what you can see.

**Two guards were wrong in ways that looked right.** Outgoing link addresses
were parsed against a placeholder base URL, so typing `example.com` produced a
link to a domain we had invented — while `javascript:` was still correctly
refused, which is what made the guard look sound. And the composer's formatting
toolbar appeared in plain-text mode, above a box its buttons could not affect,
because an author `display` rule silently defeats the `hidden` attribute. The
screenshots were byte-stable throughout: a wrongly visible toolbar is perfectly
deterministic. Byte-stability says the interface did not change, never that it
is right.

## What is new

**A right-click menu**, on the message list and in the reading pane, with one
implementation behind both. Reply, Reply all, Archive, Trash, Restore, Delete
permanently, Add sender to contacts, Verify by code, View source, Export and
Copy sender address — each offered only where it applies.

**Sent** shows who a message went to rather than who it came from, which on
outgoing mail is otherwise your own name on every row.

**Unverified mail has somewhere to go.** Previously the only action was "Accept
sender". You can now add the sender to your contacts, or verify them by code.
Adding a contact also releases their held mail, which it did not do before —
creating a contact made a sender *trusted* while leaving their mail *held and
invisible* until the sweep binned it weeks later.

**View source and export.** eeemail keeps the raw bytes of every message it
sends and receives, because the local database is the only durable copy of your
mailbox — mail is removed from the server. Until now nothing let you look at
them. Export writes a byte-exact `.eml`, not the display-safe transcoding.

**Refresh**, with a connectivity indicator, for when you do not want to wait for
the push.

**Empty trash**, behind a confirmation that says what is lost. Messages that
were never in the trash are skipped rather than destroyed, which is the safety
property the whole feature rests on.

**Formatted composing.** A toolbar over a rich-text body — bold, italic,
underline, strikethrough, headings, quotes, lists, code and links. Nothing the
browser's editor produced is sent: the body is re-emitted from a fixed tag set,
and `text/plain` is always sent alongside, derived from the same traversal so
the two cannot drift. An unformatted message still goes out as ordinary plain
text. See [ADR 0025](../docs/adr/0025-composed-html.md).

## Known gaps

**Nobody has run this build.** This is the same gap v0.3.1 shipped with, and it
is the one that matters most, because the release touches the shell and the
IPC surface. The Reply fix at the centre of it has not been clicked in a real
application. Under `xvfb` in the preparation environment the webview's web
process does not start, so what the app *draws* was again not observed.

**The three live passes were not re-run.** They need Docker and a built
`deltachat-rpc-server`, and no CI job runs them. That matters more than usual
here: `send_email` gained a sixth parameter, and yerpc compares positional
arity exactly — a five-argument call to a six-parameter method is `invalid
params`, not a defaulted `None`. All three scripts were swept by reading, not by
executing.

**The new Sent rendering is photographed nowhere.** There is no `sent.png`,
which is worth noting given that a Sent-view bug is precisely what prompted the
navigation fix above.

**`%APPDATA%` is still the Windows profile.** A SQLite mailbox and blobdir in a
roaming profile means a domain-joined machine tries to sync it at logon.
`%LOCALAPPDATA%` is right; it needs a migration and so is not in this release.

**Attachment and export filenames are reduced to a last path component.** That
stops directory traversal on both platforms, but still lets `name:stream` create
an NTFS alternate data stream and lets `CON`, `NUL` and `COM1` through.

Still unaudited, still prerelease. Use a dedicated mail account.

## Verification

Green on the tagged commit:

```
cargo nextest run --workspace --locked            1390 passed, 0 failed, 1 skipped
  ... --all-features                              1400 passed, 0 failed, 1 skipped
cargo test --workspace --locked --doc             0 failed
cargo clippy --workspace --all-targets            clean, both configs, -Dwarnings
cargo fmt --all -- --check                        clean
./scripts/check-fork-patches.sh                   clean
cd desktop && npm run check && npm run build      clean
./scripts/screenshots.sh                          11 images, byte-stable across runs
```

CI is green on this commit across all seven jobs, Windows and the test mail
server included.

**Run both nextest configurations.** `--all-features` carries ten tests the
default build does not, so a green default run is not a green CI.

## Installing

Download the installer for your platform, verify the `.sha256` beside it, and
run it. Full instructions in [`docs/INSTALL.md`](../docs/INSTALL.md).

```sh
sha256sum -c eeemail_0.4.0_amd64.deb.sha256
sudo apt install ./eeemail_0.4.0_amd64.deb
```

Or unzip `eeemail-windows-amd64.zip` / `eeemail-linux-amd64.zip` and run the app
from where it lands — on Windows, `eeemail.cmd` the first time.
[`docs/PORTABLE.md`](../docs/PORTABLE.md) is the guide, and it ships inside the
archive.

Licensed under MPL-2.0.
