# eeemail v0.3.0 — something you install

An end-to-end-encrypted email client with classic email functionality, built on
a fork of [`chatmail/core`](https://github.com/chatmail/core).

**v0.2.0 was a folder of executables.** It worked — it had been run end to end
against a real mail server — but using it meant extracting an archive and
remembering a path, and the application never said a word about what it was.
This release is the one you install: a `.deb`, an `.AppImage` or a Windows
installer, in your applications menu, that tells you on first launch what it is
and is not.

## What is new

**It installs, and it launches like an app.** `.deb` and `.AppImage` on Linux,
an NSIS installer on Windows, built by `tauri build` rather than `cargo build`.
The `.deb` and the Windows installer register a desktop entry and an icon. See
[ADR 0022](../docs/adr/0022-desktop-distribution.md).

**It says what it is, before it asks for your password.** A dialog on first
launch: this is unaudited development software; a dedicated account is
recommended and here is why; it still interoperates with ordinary mail clients;
back up the local database because there is no other copy. A `PREVIEW` marker
stays in the sidebar afterwards, because the dialog is read once and the state
it describes lasts longer than that. See
[ADR 0023](../docs/adr/0023-first-launch-disclosure.md).

**Windows worked for the first time.** `accounts_dir()` read `XDG_DATA_HOME` and
then `HOME` and gave up if it found neither, which is the ordinary state of a
Windows session — so the Windows binary the release matrix had been building
since Phase 7 exited on launch. No test could have caught it: there is no Windows
runner in CI, and the function reads the environment rather than anything a unit
test constructs.

**`Holding` is now `Unverified`,** everywhere: the label, the sidebar, the RPC
wire, the stored row. "Holding" described what the mailbox was doing;
"Unverified" describes what is true about the sender, which is the thing you have
to decide about. Migration 171 renames the label in place, so no message loses
its tag.

**One thing destroys mail on a timer, and it is Trash.** Unverified mail that
was never accepted used to be destroyed outright at 30 days, while the deadline
a few lines away in the same housekeeping pass had already grown a recoverable
window on the argument that a timer must not destroy the only copy of a mailbox.
That argument does not stop applying because the mail came from a stranger. So:

| Route in | What happens |
|---|---|
| Unverified, never accepted | Moves to **Trash** after 30 days *(never / 7 / 30 / 90)* |
| A disappearing-message timer fires | Moves to **Trash**, unchanged since v0.2.0 |
| You throw it away | Moves to **Trash** |
| Anything in Trash | **Destroyed** after 30 days *(immediately / 7 / 30 / 90)* |

Both windows are now settings rather than one constant and one setting, and the
unverified window is measured from when the message was held and read afresh on
every sweep — so shortening it moves mail that is already waiting. The reading
pane says which of the three reasons put a message in Trash.

**Full install and run instructions.** [`docs/INSTALL.md`](../docs/INSTALL.md):
verifying the download, first launch, the dedicated-account recommendation and
how to share an account anyway, interoperability with everyone else's client,
where your mail is stored, the deadlines, uninstalling, and what the two
command-line tools are for.

## Things to know before trusting it

- **Nothing here is signed.** SmartScreen will warn on Windows and Linux desktops
  that check signatures will say so. The `.sha256` beside each file proves the
  download arrived intact; it does not prove it came from us.
- **Keys learned from an `Autocrypt:` header are not authenticated.** Anyone who
  can write the `From` line of a first message can write the `Autocrypt` line.
  That protects you from someone reading stored mail; it does **not** protect you
  from someone rewriting mail in flight. That is Autocrypt's own threat model,
  and it is why "encrypted" and "verified" are two separate badges in the UI —
  only a QR verification survives an active attacker. See
  [ADR 0021](../docs/adr/0021-autocrypt-key-contacts.md).
- **Interop is proven against Delta Chat's engine and GnuPG, and nothing else.**
  `scripts/interop-pass.py` runs eeemail against upstream's released
  `deltachat-rpc-server` — the same binary Delta Chat Desktop ships — and
  `scripts/gpg-interop-pass.py` has GnuPG decrypt our PGP/MIME and verify our
  signatures. **Thunderbird, Gmail and every mainstream provider remain
  untested.** That is the largest gap in the project.
- **A stock Delta Chat will not accept your first message.** It ships with
  `force_encryption` on, which refuses to send *or download* cleartext, so the
  first message — which has no key to use yet — is dropped before its `Autocrypt`
  header can be read. The other end has to turn that off once.
- **Blob encryption is opt-in and needs a database passphrase.** Until you set
  one, attachments and retained message sources stay in cleartext in the blobdir.
  The app reports what is and is not protected rather than claiming otherwise.
- **Encrypted mail can silently omit a recipient** whose key is missing —
  upstream behaviour we surface rather than change. eeemail records who never
  received it.
- **macOS is not built.** The path handling is there; nothing has been compiled
  or tested on it, and an untested `.dmg` is worse than an absent one.
- **Camera QR scanning is not wired up.** Paste and file are the working paths.
- **One attachment per message**, because core carries one file per message. The
  composer says so rather than hiding it.
- Most of this was written by a large language model under human direction. It is
  reviewed and tested; it has **not** been audited by a security professional,
  and an encrypted mail client is exactly the kind of software where that
  distinction matters. Do not rely on it for anything that matters yet.

## Verification

```
cargo nextest run --workspace              1365 passed, 0 failed, 1 skipped
cargo test --workspace --locked --doc      0 failed
cargo clippy --workspace --all-targets     clean, default and --all-features
cargo fmt --all -- --check                 clean
scripts/check-fork-patches.sh              clean
desktop: npm run check, npm run build      clean
scripts/screenshots.sh                     11 images, byte-stable across runs
server/compose/smoke-test.py               all checks pass
python3 scripts/e2e-pass.py                all six steps pass, live
python3 scripts/interop-pass.py            all steps pass, against upstream v2.59.0
python3 scripts/gpg-interop-pass.py        all steps pass, against GnuPG 2.4.9
```

Clippy runs with `-Dwarnings`, in both feature configurations, because
`--all-features` alone never lints the default build — which is the one we ship.

The live passes cover the rename and the new default across the wire, not only
in Rust: `e2e-pass.py` step 1 asserts `get_unverified_trash_days` is 30, and
steps 3 and 5 of the interop pass drive the `Unverified` tag through JSON-RPC
against a stock Delta Chat engine.

The `.deb` was built and unpacked to check it: `usr/bin/eeemail`, a
`usr/share/applications/eeemail.desktop` with `Exec=eeemail`, and icons at three
sizes. The `.AppImage` could not be built where this was prepared —
`linuxdeploy-plugin-gtk` hardcodes a gdk-pixbuf path that modern distributions
no longer use — so it is the one artefact that has never been produced.

**Three things no test suite covers, because none can be reached from CI.**
All must be done before the tag:

0. **Rehearse the release with `workflow_dispatch`**, which builds and uploads
   artefacts without cutting a tag, and confirm the `.AppImage` is among them.

1. **Install each artefact on a clean machine** and launch **from the
   applications menu** — a launcher entry is the thing being tested, so starting
   it from a terminal proves nothing. Confirm the first-launch dialog appears,
   dismiss it, set up an account, send and receive, relaunch, and confirm the
   dialog does not come back.
2. **On Windows, confirm the account directory is under `%APPDATA%\eeemail`.**
   This is the path that could not work before v0.3.0, and there is no Windows
   runner in CI to check it.

## Installing

Download the installer for your platform, verify the `.sha256` beside it, and
run it. Full instructions in [`docs/INSTALL.md`](../docs/INSTALL.md).

```sh
sha256sum -c eeemail_0.3.0_amd64.deb.sha256
sudo apt install ./eeemail_0.3.0_amd64.deb
```

The `eeemail-*.tar.gz` and `.zip` archives hold the two command-line tools,
`eeemail-cli` and `deltachat-rpc-server`. Neither is needed to use eeemail and
the app uses neither — it embeds the engine in-process.

Licensed under MPL-2.0.
