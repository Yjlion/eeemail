# 0024 — The release archive holds the app, not only the tools

**Status:** Accepted — 2026-09-04
**Amends:** [0022](0022-desktop-distribution.md), on what the archive contains

## Context

[ADR 0022](0022-desktop-distribution.md) decided that eeemail ships as a
platform installer rather than as a folder of executables, and that the archive
"stays and becomes the *tools*": `eeemail-cli` and `deltachat-rpc-server`,
neither of which the app uses. The reasoning holds and is not in question here
— a `.deb` registers a desktop entry and an icon, and nobody should have to
remember a path to read their mail.

What it left behind is that **there is no way to run eeemail without installing
it**. `docs/INSTALL.md` describes the archive as "the command-line tools, which
most people do not need", which is accurate and is also the whole problem: the
GUI binary that `tauri build` already produces, sitting in the same directory as
the two tools, was never copied into it.

Three things forced this.

**Someone installed v0.3.0 and it did not work.** The desktop app had no
`capabilities/` directory, so Tauri's ACL compiled to `{}` and `listen()` was
refused: the frontend attached no event stream and every RPC call hung. The app
was dead on arrival on every platform. Reproducing that took an installation,
because installing was the only way to run it — and the reporter's screenshot
was, for a while, the only evidence anyone had.

**Nothing automated exercises the app.** `scripts/screenshots.sh` renders the
demo build in a browser, with no Tauri IPC at all. `scripts/e2e-pass.py` drives
`deltachat-rpc-server`, which the app does not use. ADR 0022 records that "no
test could have caught" the `accounts_dir()` bug; the same sentence turned out
to be true of the ACL. Until the real binary is easy to obtain and run, the gap
between "green CI" and "the app works" is crossed by hand or not at all.

**"Install this to try it" is a real cost.** eeemail is unaudited prerelease
software that asks for a mail password. Telling someone to install it before
they can look at it is a poor trade, and a `.deb` or an NSIS installer is not
something a cautious person undoes casually.

## Decision

**The release archive holds all three binaries — the app first — and can be
unzipped and run.** One `.zip` per platform; the Linux `.tar.gz` becomes a
`.zip` so both platforms produce the same kind of file.

This is *beside* the installers, not instead of them. ADR 0022's channel remains
the recommended one and `INSTALL.md` still says so.

**A portable copy keeps its profile beside the executable.** An empty file named
`eeemail-portable` ships inside the archive and nowhere else; when `data_dir()`
finds it next to the running executable, the whole profile — accounts, staged
attachments, the first-launch marker — goes in `data/` in that folder instead of
`%APPDATA%\eeemail` or `~/.local/share/eeemail`.

The marker is a file rather than a build flag, a separate binary or an
environment variable, because those all mean two artefacts to build, sign, test
and confuse. One executable, and what makes it portable is where it was
unpacked.

**The Windows archive carries the WebView2 bootstrapper**, with an `eeemail.cmd`
that installs the runtime if the machine lacks it and then starts the app. The
NSIS bundle is set to `embedBootstrapper` for the same reason.

## Consequences

**Trying eeemail no longer means installing it**, and neither does reproducing a
bug in it. That is the point.

**The portable and installed copies cannot see each other's mail.** They use
different directories by construction. This is right — two copies sharing one
SQLite mailbox is worse — but it will surprise someone who sets up an account in
the archive and then installs.

**Deleting the folder deletes the mailbox.** The local database *is* the
mailbox; the server has nothing left to re-download. Ordinarily the profile
outlives the application, and here it does not. `PORTABLE.md` says so twice.

**The Linux archive is portable but not self-contained.** The `eeemail` ELF
links system webkit2gtk and libsoup at runtime. On a machine without them it
will not start, and the `.AppImage` — which carries them — remains the answer.
Two words that sound alike and mean different things, so the docs use both
deliberately.

**The Windows release leg gains a second unpinned third-party download.**
`docs/handoff.md` records the v0.3.0 tag build failing on the first one
(`nsis-3.zip: Connection Failed`) and calls it "a coin flip … on every Windows
release". The Evergreen WebView2 bootstrapper is versionless by design and so
cannot be hash-pinned. The alternative — downloading it on the user's machine at
first run — was considered and rejected: it moves a flaky download from a job
that can be re-run to a person who cannot re-run it. Mitigated with three
retries and a check that what came back is a PE rather than an error page.

**`deltachat-rpc-server` is in the archive beside the app again**, which ADR
0022 removed precisely because it "implied a dependency that does not exist".
The archive's `PORTABLE.md` states in its file table that the app does not use
it. That is a weaker guard than absence, and it is the price of one archive
rather than two.

**Nothing is signed**, on either channel. The `.sha256` beside each file proves
the download is intact, not that it is ours.
