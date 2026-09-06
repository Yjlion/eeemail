# eeemail v0.3.1 — the one that runs

An end-to-end-encrypted email client with classic email functionality, built on
a fork of [`chatmail/core`](https://github.com/chatmail/core).

**v0.3.0 did not work.** It installed correctly on every platform and then
failed the moment it opened a window, with `Command plugin:event|listen not
allowed by ACL` where the mailbox should have been. This release fixes that, and
changes how the project is distributed so that the next one like it is found
before a tag rather than after.

## What is fixed

**The desktop app has an ACL again, so it can hear the engine.** Tauri 2
resolves its permissions by globbing `src-tauri/capabilities/` at build time.
That directory did not exist, so the ACL compiled to `{}`. The four commands the
shell registers itself are not ACL-checked and still worked — but `listen()` is
the *core event plugin*, and it is. The frontend attaches the `rpc-message`
stream in its constructor and awaits it before every call, so nothing ever
resolved: no mail, no accounts, no events, on Linux and Windows alike.

**Why nothing caught it, which is the more useful half.** Nothing automated in
this repository exercises the real Tauri IPC path. `scripts/screenshots.sh`
photographs a browser-only demo build that answers from fixtures and never calls
`invoke`. `scripts/e2e-pass.py` drives `deltachat-rpc-server`, which the app does
not use — the shell embeds the engine in-process. Every CI job ran on
`ubuntu-latest`, and the Windows leg of the release workflow compiled and
packaged without executing anything. 1375 tests, three live passes and eleven
byte-stable screenshots were all green against an application that could not
open a mailbox.

Three guards were added rather than one fix:

- CI and the release workflow now assert that the **generated**
  `gen/schemas/capabilities.json` grants an event permission. That the source
  file exists proves nothing about what got compiled in, and it was the compiled
  artefact that was empty.
- **A Windows CI job.** The release matrix has built Windows since Phase 7 and
  has never run anything on it. That is the structural reason `accounts_dir()`
  could be wrong for eight releases and the ACL could be empty for one.
- **The desktop shell has unit tests**, its first. `data_dir()` was the function
  that broke Windows in v0.3.0 and it had no test at all, because it was written
  with `#[cfg]` and so could only ever be checked on the platform it was wrong
  about. The platform is now a parameter, and the Windows rule is tested from
  Linux.

**A failed launch on Windows now says why.** A release build is
`windows_subsystem = "windows"` and so has no console: the error returned from
`main` and every `eprintln!` went to a stderr that did not exist. That is how the
original `accounts_dir()` bug survived eight releases — the app simply did not
appear, and there was nothing to report. It now shows a message box carrying the
error chain.

## What is new

**A zip you unzip and run.** `eeemail-windows-amd64.zip` and
`eeemail-linux-amd64.zip` now hold the app and both command-line tools, and run
from wherever you unpack them. v0.3.0's archives held only the tools, which
meant there was no way to run eeemail without installing it — and so no way to
try it, or to reproduce a bug in it, on a machine you would rather not install
onto. Reproducing the bug above required an installation, which is what forced
this. The Linux `.tar.gz` becomes a `.zip` so both platforms ship the same kind
of file. See [ADR 0024](../docs/adr/0024-portable-archives.md).

**A portable copy keeps its mail beside the executable.** An empty
`eeemail-portable` file ships inside the archive and nowhere else; when the app
finds it next to itself, the whole profile — accounts, staged attachments, the
first-launch marker — goes in `data/` in that folder rather than in
`%APPDATA%\eeemail` or `~/.local/share/eeemail`. One executable, and what makes
it portable is where it was unpacked. Unzip it, try it, delete the folder:
nothing is left behind, and it cannot collide with an installed copy.

**The Windows archive carries the WebView2 runtime.** eeemail draws its window
with WebView2, and only the installer bootstrapped it — so an unzipped copy on a
fresh VM, an LTSC or N edition, or a machine that has never run Edge would start
and vanish with no message. `eeemail.cmd` checks for the runtime, installs it
from the bundled bootstrapper if it is absent, and launches the app. The NSIS
bundle now embeds the bootstrapper too, rather than downloading it at install
time.

**The installers are unchanged and remain the recommended way in.** ADR 0022's
reasoning holds: a `.deb` or an NSIS installer registers a desktop entry and an
icon, and nobody should have to remember a path to read their mail. The archive
is a second channel, not a replacement.

## Known gaps

- **Nothing automated still exercises the real desktop IPC path.** The guards
  above catch the specific shape of this bug — an empty ACL — and not the next
  one. Running the shell by hand after touching it is the only check there is,
  and `CLAUDE.md` and `docs/development.md` now say so.
- **Interop with Thunderbird, Gmail or any mainstream provider is untested.**
  Delta Chat's own engine and GnuPG are covered by `scripts/interop-pass.py` and
  `scripts/gpg-interop-pass.py`; everything else is not, and is not reachable
  from this environment.
- **The Linux archive is portable but not self-contained.** The `eeemail` binary
  links the system webview and needs `libwebkit2gtk-4.1`, `libsoup-3.0` and GTK
  3 installed. The `.AppImage` carries them and is the build that needs nothing.
- **The Windows release leg now has a second unpinned third-party download.**
  The v0.3.0 tag build failed once on the first (`nsis-3.zip: Connection
  Failed`). The Evergreen WebView2 bootstrapper is versionless by design and so
  cannot be hash-pinned; the fetch retries three times and checks that what came
  back is a Windows executable rather than an error page.
- **`%APPDATA%` is the roaming profile,** and a SQLite mailbox does not belong
  there on a domain-joined machine. Moving it to `%LOCALAPPDATA%` is the right
  change and needs a migration, so it is not in a fix release.
- **Encrypted mail can silently omit a recipient** whose key is missing —
  upstream behaviour we surface rather than change.
- **macOS is not built.** **Camera QR scanning is not wired up.** **One
  attachment per message.**
- Most of this was written by a large language model under human direction. It is
  reviewed and tested; it has **not** been audited by a security professional,
  and an encrypted mail client is exactly the kind of software where that
  distinction matters. Do not rely on it for anything that matters yet.

## Verification

```
cargo nextest run --workspace --locked              1376 passed, 0 failed, 1 skipped
  ... --all-features                                1386 passed, 0 failed, 1 skipped
cargo test --workspace --locked --doc               0 failed
cargo clippy --workspace --all-targets              clean, default and --all-features
cargo fmt --all -- --check                          clean
scripts/check-fork-patches.sh                       clean
desktop: npm run check, npm run build               clean
scripts/screenshots.sh                              11 images, byte-stable across runs
```

Eleven more tests than v0.3.0 in each configuration (1365 and 1375), and all
eleven are the desktop shell's, which had none. Clippy runs with `-Dwarnings` in
both configurations, because `--all-features` alone never lints the default
build — which is the one we ship.

**The ACL bug was reproduced before it was fixed, and by the test that now
guards it.** `test_the_frontend_is_allowed_to_hear_the_engine` builds the real
`generate_context!()` and asks the compiled `RuntimeAuthority` whether
`plugin:event|listen` resolves for the `main` window — the same authority the
running app consults, not a re-reading of the source file. With
`capabilities/` moved aside it fails with `plugin:event|listen is refused for
the `main` window`; with it in place it passes. That is the diagnosis confirmed
rather than assumed.

**The portable rule was checked on the real binary.** Copied into a scratch
folder beside an `eeemail-portable` marker and launched, it put its profile in
`<folder>/data/accounts/` and wrote nothing to `~/.local/share/eeemail`.

**The three live passes were not re-run.** `e2e-pass.py`, `interop-pass.py` and
`gpg-interop-pass.py` exercise the engine, and the engine did not change: the
entire diff to `core/` in this release is two version numbers in `Cargo.lock`.
They passed on v0.3.0 and CI runs the suite that covers the same code.

**What was not verified, and it is the same gap that shipped v0.3.0 broken.**
Nothing automated exercises the real Tauri IPC path, and the environment this
was prepared in could not either — WebKitGTK's web process does not start under
`xvfb`, so the webview never executed any JavaScript. The app starts, opens its
accounts directory and stays running; **what it draws was not observed.** Nobody
has seen v0.3.1 render a mailbox.

**Two things no test suite covers.** v0.3.0 went out with the same two
outstanding and that is exactly how it shipped broken:

1. **Install each artefact and launch it from the applications menu**, on Linux
   and on Windows. Dialog appears, dismiss it, set up an account, send and
   receive, relaunch, dialog stays gone.
2. **Unzip the archive on Windows and run `eeemail.cmd`.** Window appears with
   no ACL banner; the profile is in `data\` inside the unzipped folder; an
   installed copy on the same machine still uses `%APPDATA%\eeemail` and does
   not see the portable one's mail.

## Installing

Download the installer for your platform, verify the `.sha256` beside it, and
run it. Full instructions in [`docs/INSTALL.md`](../docs/INSTALL.md).

```sh
sha256sum -c eeemail_0.3.1_amd64.deb.sha256
sudo apt install ./eeemail_0.3.1_amd64.deb
```

Or unzip `eeemail-windows-amd64.zip` / `eeemail-linux-amd64.zip` and run the app
from where it lands — on Windows, `eeemail.cmd` the first time.
[`docs/PORTABLE.md`](../docs/PORTABLE.md) is the guide, and it ships inside the
archive.

Licensed under MPL-2.0.
