# eeemail v0.6.0 — the one where the composer is an editor

An end-to-end-encrypted email client with classic email functionality, built on
a fork of [`chatmail/core`](https://github.com/chatmail/core).

**Formatted mail is now written in a real editor.** The composer's formatted
mode was a bare `contenteditable` driven by `document.execCommand` — deprecated,
and different on each of the three webviews eeemail ships on. It is now
[Squire](https://github.com/fastmail/Squire), Fastmail's email editor, and the
toolbar gains text colour, highlight, font, size and alignment.

This release is frontend only. The engine, the RPC surface and `send_email` are
unchanged from v0.5.0.

## What is new

**Squire underneath the toolbar.** Bold, italic, underline, strikethrough,
heading, quote, lists, code and link now toggle off as well as on, show as
pressed where the caret is, and undo as one step. The usual shortcuts work:
Ctrl+B/I/U, Ctrl+Shift+7/8/9, Ctrl+[ and ], Ctrl+D for code, Ctrl+Z.

**Colour, highlight, font, size and alignment.** Colours are free; fonts are
sans-serif, serif or monospace; sizes are small, large and huge; alignment is
left, centre, right or justified. Fonts and sizes are deliberately few: a named
font is a guess about what the recipient has installed, and a keyword size
scales with their default where a pixel size does not.

**A paste shows what will be sent.** What you paste goes through the same filter
as the message itself, so formatting that would be stripped on send is stripped
on paste, where you can see it. There is one filter, not a paste sanitiser and a
send sanitiser that can disagree. The first parse of pasted HTML is inert and
never enters the document; only the filtered result does.

**What goes on the wire is still rebuilt, never copied.** The whitelist gains
`<span>` and a `style` attribute written from checked properties only — a colour
with nothing in it but a colour, a family from the list, a size from the list, an
alignment from four. `class` is never sent. `text/plain` is still always sent
beside the HTML, and a message with no formatting still goes out as plain mail.
[ADR 0030](../docs/adr/0030-the-composer-edits-with-squire.md) records the
decision and amends [0025](../docs/adr/0025-composed-html.md).

## What was wrong before it shipped

Two style checks passed review and failed the first time they ran in a browser.
`color: red` stays the keyword `red` rather than becoming `rgb()`, so a check
that only knew `rgb()` stripped every named colour. And a pasted `background:`
shorthand leaves `background-color: initial`, which a letters-only check took
for a colour. Both are fixed. The lesson is in `docs/handoff.md`: anything that
reads a parsed style value has to be checked against a browser, not against
what the value was set to.

## Known gaps

**Nobody has run this build**, now for the fourth release in a row — and this
one changes the part of the app a person types into. The editor was driven in
headless Chromium, which is none of the engines eeemail ships on: nothing was
run in WebKitGTK or WebView2. Squire is built for all three, but that is its
claim, not something observed here.

**The sanitiser has no automated test.** It is the most security-relevant code
in the frontend. It was checked with 30 cases in a throwaway harness — `onerror`,
`<script>` and `<style>` text, `javascript:` links, `url()` in styles, free-text
fonts, attribute injection, a whole-document paste — and none of that is
committed, because the frontend has no test runner.

**A recipient may not show the new styles.** Delta Chat's clients converged on a
tag list without `<span>`; a client that ignores it shows the words unstyled.
Bold pasted from Google Docs, which marks it with a style rather than a tag,
arrives as plain text.

**No styled message has been sent through a real server and read back.** The
live passes were not re-run, and none of them composes formatted mail.

**eeemail now has a rich-text library in the process that renders mail.** It
never sees received mail — message HTML still renders only in the sandboxed
frame — and it is pinned exactly. It is still a dependency where v0.5.0 had
none.

**`%APPDATA%` is still the Windows profile**, attachment filenames are still only
reduced to a last path component, and the trash notice still reads the wall
clock. All unchanged from v0.5.0.

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

CI is green across all seven jobs, Windows and the test mail server included,
on `6e4d884f8`, the merge of the release PR. The tagged commit differs from it
only in this paragraph, and was tagged after its own CI run passed.

Three of the thirteen screenshots do not match the committed bytes when
regenerated here. `trash.png` and `trash-swept.png` read the wall clock, which
is known. `contact-detail.png` differs with no visible change, most likely from
a newer Chromium than the one that rendered the committed images. A diff in
those images is not by itself a change in the UI.

## Installing

Download the installer for your platform, verify the `.sha256` beside it, and
run it. Full instructions in [`docs/INSTALL.md`](../docs/INSTALL.md).

```sh
sha256sum -c eeemail_0.6.0_amd64.deb.sha256
sudo apt install ./eeemail_0.6.0_amd64.deb
```

Or unzip `eeemail-windows-amd64.zip` / `eeemail-linux-amd64.zip` and run the app
from where it lands — on Windows, `eeemail.cmd` the first time.
[`docs/PORTABLE.md`](../docs/PORTABLE.md) is the guide, and it ships inside the
archive.

Licensed under MPL-2.0.
