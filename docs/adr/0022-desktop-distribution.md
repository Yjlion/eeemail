# 0022 — eeemail ships as a platform installer, not as a folder of executables

**Status:** Accepted — 2026-09-03

## Context

Through v0.2.0 a release was a `tar.gz` or a `zip` containing three loose
executables — `eeemail-cli`, `eeemail-desktop`, `deltachat-rpc-server` — plus
the licence and the README. The instructions were "extract it".

That is a fine way to ship a developer tool and a poor way to ship a mail
client. Nothing registered a desktop entry, so the app did not appear in any
launcher or menu; nothing installed the icon that had been sitting in
`desktop/src-tauri/icons/` since Phase 7; and the person who extracted the
archive had to remember a path forever. `tauri.conf.json` had declared
`"bundle": { "active": true, "targets": "all" }` the whole time and nothing ever
invoked the bundler, because the release workflow ran plain `cargo build`.

Two things made this worth fixing now rather than later. The release matrix had
been building a Windows executable that **could not start**: `accounts_dir()`
read `XDG_DATA_HOME` and then `HOME`, and returned an error when it found
neither, which is the ordinary state of a Windows session. And the archive
shipped `deltachat-rpc-server` next to the desktop app in a way that implied the
app needed it, which it does not — the app embeds the engine in-process
([0013](0013-desktop-ui.md)).

## Decision

**The desktop app ships as a platform installer.** `.deb` and `.AppImage` on
Linux, an NSIS installer on Windows, built by `tauri build` rather than
`cargo build`. The `.deb` and the NSIS installer put eeemail in the applications
menu with its icon, which is the whole point.

**The archive stays, and becomes the tools.** It now holds `eeemail-cli` and
`deltachat-rpc-server` and says what each is for. Neither is needed to use
eeemail and the app uses neither.

**Bundle targets are passed per platform on the command line**, not taken from
the config's list. `targets: "all"` on a Linux runner asks for an `.rpm` as
well, and a release must not fail over a bundle format nobody asked for and no
runner has the tooling for.

**`data_dir()` branches per platform** — `%APPDATA%` on Windows,
`~/Library/Application Support` on macOS, XDG otherwise — and `accounts_dir()`
and the first-run marker are both derived from it.

**macOS is not built.** The path handling is there and the code has no
Apple-specific parts, but nothing has been compiled or tested on it, so no
artefact is published. An untested `.dmg` is worse than an absent one.

## Consequences

- **The app is discoverable.** Install it and it is in the menu, which is the
  difference between software people use and software people evaluate.
- **The `.deb` was built and inspected; the `.AppImage` was not.** A
  `tauri build --debug --bundles deb` produces `usr/bin/eeemail`, a
  `usr/share/applications/eeemail.desktop` with `Exec=eeemail` and
  `Icon=eeemail`, and icons at three sizes — which is the launcher entry this
  ADR exists for, verified rather than assumed. AppImage bundling could not be
  run here: `linuxdeploy-plugin-gtk` hardcodes `/usr/lib/gdk-pixbuf-2.0/2.10.0`,
  a path modern gdk-pixbuf no longer uses, so it fails on the developer machine
  for reasons that have nothing to do with this project. It has to be checked on
  the first `workflow_dispatch` rehearsal instead.

- **`category` is `Productivity`, which becomes `Categories=Office;`.** Tauri
  accepts a fixed set of app categories and "Email" is not among them. `Office`
  is where Thunderbird tends to appear too, so it is defensible rather than
  ideal; getting `Network;Email;` would mean maintaining a `desktopTemplate`,
  which is a whole file to keep in step with Tauri's default for a menu
  heading.

- **Nothing is signed.** SmartScreen will warn on Windows, and Linux desktops
  that check signatures will say so. The `.sha256` beside each file proves the
  download arrived intact and nothing more; it is not a substitute for signing,
  and `INSTALL.md` says so rather than implying otherwise. Code signing needs
  certificates and an identity this project does not yet have.
- **The AppImage does not appear in a launcher on its own.** That is what an
  AppImage is, and the honest answer is to point people who want a menu entry at
  the `.deb` rather than to explain desktop integration.
- **A release now takes longer**, because `tauri build` runs after the cargo
  build rather than instead of part of it. The 120-minute job timeout already
  accommodates it, and the two share a target directory and a `rust-cache` key.
- **Windows works for the first time.** The old `accounts_dir()` had been
  shipping in an executable that exited on launch since Phase 7, and no test
  could have caught it: there is no Windows runner in CI and the function reads
  the environment rather than anything a unit test constructs. Verifying this
  needs a real install on a real Windows machine, which
  [`INSTALL.md`](../INSTALL.md) and the release checklist both say.
- **The npm and Rust Tauri versions now have to stay in lockstep**, and are
  pinned to exact versions rather than caret ranges to make that hold.
  `tauri build` refuses to run when the `tauri` crate and `@tauri-apps/api`
  differ in major/minor; `cargo build` never checked, so the two had already
  drifted to 2.2.5 and 2.11.1 without anything noticing. Switching to
  `tauri build` is what surfaced it, and it would have failed the first real
  release rather than a test.

  The direction of the fix was forced: no `tauri` crate newer than 2.2.5
  resolves under this workspace's `rust-version = 1.89`, and CI gates on that
  MSRV, so the npm side came down to meet the Rust side rather than the other
  way about. Raising the MSRV to move Tauri forward is a real decision and
  belongs in its own change, not smuggled into a packaging one.

- **`deltachat-rpc-server` stops looking load-bearing.** It was next to the app
  in every archive, which reads as a dependency. It is not one, and the release
  notes and install guide now say what it is actually for.
