# Running eeemail without installing it

This archive is the whole of eeemail: the app, and the two command-line tools.
Unzip it anywhere you can write, run it, and delete the folder when you are
done. Your mail, your accounts and your settings stay inside the folder, and
nothing is registered with the operating system.

(One exception, on Linux only, and it is not ours: the system webview keeps a
small HSTS cache at `~/.local/share/eeemail/hsts-storage.sqlite`. It holds no
mail — it is a list of hosts that asked for HTTPS — and WebKitGTK picks that
path from the program name before eeemail has any say in it.)

If you want eeemail in your applications menu instead, use an installer and
read [`INSTALL.md`](INSTALL.md). This archive is for trying it, for testing a
release, and for running it on a machine you would rather not install onto.

## What is in here

| File | What it is |
|---|---|
| `eeemail` / `eeemail.exe` | The app. This is the one you want. |
| `eeemail.cmd` | Windows only. Installs the WebView2 runtime if it is missing, then starts the app. |
| `MicrosoftEdgeWebview2Setup.exe` | Windows only. Microsoft's WebView2 installer, used by `eeemail.cmd`. |
| `eeemail-cli` | Inspects a mailbox. It cannot send or receive — it never starts the engine's IO loop. |
| `deltachat-rpc-server` | The engine over JSON-RPC, for automation and the test scripts. **The app does not use it**; it embeds the engine in-process. |
| `eeemail-portable` | An empty file. Its presence is what makes this copy portable — see below. |

## Windows

Run **`eeemail.cmd`** the first time.

eeemail draws its window with WebView2. Windows 11 and up-to-date Windows 10
have it; a fresh VM, an LTSC or N edition, or a machine that has never run Edge
may not. The installer carries the runtime, so an installed copy never has to
think about it — but an unzipped copy does, and without the runtime the app
starts and then disappears with no message. `eeemail.cmd` checks for the
runtime, installs it from the bundled `MicrosoftEdgeWebview2Setup.exe` if it is
absent, and then launches `eeemail.exe`.

Once it has run successfully you can start `eeemail.exe` directly.

Nothing here is code-signed, so SmartScreen will say the publisher is unknown.
"More info" → "Run anyway" is the way past it. Check the `.sha256` beside the
download first: that is the only integrity check this project offers.

## Linux

Run `./eeemail`.

You may need to make it executable first — some unzip tools drop the bit:

```sh
chmod +x eeemail eeemail-cli deltachat-rpc-server
./eeemail
```

**The Linux binary is not self-contained.** It links the system webview at
runtime, so it needs `libwebkit2gtk-4.1`, `libjavascriptcoregtk-4.1`,
`libsoup-3.0` and GTK 3 to be installed. On Debian and Ubuntu:

```sh
sudo apt install libwebkit2gtk-4.1-0 libsoup-3.0-0 libgtk-3-0
```

If you would rather not install anything at all, use the **`.AppImage`** from
the same release page: it carries those libraries with it and is the genuinely
self-contained Linux build. This archive is the portable one; the AppImage is
the self-contained one, and they are not the same thing.

## Where your mail lives

Inside this folder, in `data/`.

That is the difference between a portable copy and an installed one, and it is
decided by the empty `eeemail-portable` file sitting beside the executable. If
it is there, the whole profile — accounts, staged attachments, the first-launch
marker — goes in `data/` next to the app. If it is not, eeemail uses the
platform's usual location (`%APPDATA%\eeemail`, `~/.local/share/eeemail`,
`~/Library/Application Support/eeemail`).

Two things follow, and both are the point:

- **A portable copy and an installed copy never share a profile.** They can sit
  on one machine and neither will see the other's mail.
- **Deleting this folder deletes your mail.** The local database *is* the
  mailbox — eeemail removes mail from the server once it has it, so there is
  nothing left to re-download. If you set up a real account here rather than a
  test one, back `data/` up.

`EEEMAIL_ACCOUNTS_DIR` still overrides where accounts are read from, which is
how the test scripts run against a scratch profile.

## If the window never appears

**Windows.** Almost always the WebView2 runtime — run `eeemail.cmd`. If it
still fails, eeemail now shows a message box saying why rather than exiting
silently; that text is the thing to report.

**Linux.** Run it from a terminal, where it will say what is wrong:

```sh
./eeemail
RUST_LOG=info ./eeemail    # for more
```

`error while loading shared libraries` means a missing webview package, above.

## What this is

eeemail is **unaudited** and this is a **prerelease**. Use a dedicated mail
account rather than your main one. It interoperates with ordinary mail clients
and has been run against Delta Chat's engine and GnuPG, but not against
Thunderbird, Gmail, or any mainstream provider. The `README.md` beside this file
has the full status, and so does
[the project page](https://github.com/Yjlion/eeemail).
