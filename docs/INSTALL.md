# Installing and running eeemail

**Read this first:** eeemail is development software. It is reviewed and tested,
most of it was written by a large language model under human direction, and it
has **not** been audited by a security professional. An encrypted mail client is
exactly the kind of software where that distinction matters. Do not rely on it
for anything consequential yet. The application says the same thing the first
time you launch it.

## What you download

The [release page](https://github.com/Yjlion/eeemail/releases) has two kinds of
file per platform.

| File | What it is |
|---|---|
| `eeemail_0.3.0_amd64.deb`, `eeemail_0.3.0_amd64.AppImage` | The app, for Linux |
| `eeemail_0.3.0_x64-setup.exe` | The app, for Windows |
| `eeemail-linux-amd64.tar.gz`, `eeemail-windows-amd64.zip` | The command-line tools, which most people do not need |

You want the app. The archive is covered at the bottom.

### Verify what you downloaded

Every file has a `.sha256` beside it. Checking it costs one command and means a
corrupted or substituted download fails loudly rather than quietly:

```sh
sha256sum -c eeemail_0.3.0_amd64.deb.sha256
```

On Windows, in PowerShell:

```powershell
(Get-FileHash .\eeemail_0.3.0_x64-setup.exe -Algorithm SHA256).Hash
Get-Content .\eeemail_0.3.0_x64-setup.exe.sha256
```

The two hashes must match. Note what this does and does not do: it proves the
file arrived intact, not that it came from us. Nothing here is code-signed, so
Windows SmartScreen and any Linux desktop that checks signatures will say so.

## Install

**Debian, Ubuntu and derivatives.**

```sh
sudo apt install ./eeemail_0.3.0_amd64.deb
```

eeemail then appears in your applications menu. Launch it there, or run
`eeemail` from a terminal.

**Any other Linux — the AppImage.**

```sh
chmod +x eeemail_0.3.0_amd64.AppImage
./eeemail_0.3.0_amd64.AppImage
```

An AppImage installs nothing and runs from wherever you put it. It will *not*
appear in your applications menu unless you integrate it yourself, with
something like [Gear Lever](https://github.com/mijorus/gearlever) or
`appimaged`. If you want a launcher entry without thinking about it, use the
`.deb`.

**Windows.** Run `eeemail_0.3.0_x64-setup.exe` and follow the installer.
eeemail then appears in the Start menu. SmartScreen will warn that the publisher
is unknown, because the installer is unsigned; "More info" → "Run anyway" is the
way past it, and you should have checked the hash above before deciding to.

**macOS** is not built yet. The code has no Apple-specific parts and the data
directory is already handled, but nothing has been compiled or tested there, so
there is nothing to download.

## First launch

The first thing you see is a dialog saying what this software is. It is the same
warning as the top of this file, and it appears once.

Then the setup form asks for an email address and a password. If your provider
is in Thunderbird's autoconfiguration database — most are — that is all it
needs; **Server settings** is there for the ones that are not, and for a test
server.

> **Accept an invalid certificate** is for a test server with a self-signed
> certificate and nothing else. On a real account it removes the protection that
> stops somebody else answering for your provider, which is most of what TLS was
> for. Leave it off.

## Use a dedicated account

This is the recommendation, and here is the reason rather than just the advice.

eeemail uses IMAP and SMTP as **transport only**. It downloads mail, decrypts
it, stores it on your device, and then **deletes it from the server**. The local
database becomes your mailbox; the server becomes a spool with nothing left in
it. That is a deliberate design decision — see
[ADR 0003](adr/0003-imap-as-transport.md) and
[ADR 0004](adr/0004-local-store-and-raw-mime.md) — and it is why a fresh account
is the comfortable way to try this.

Point eeemail at the account you already read in Thunderbird or webmail and it
will drain that account into this one device.

**If you want to share one account anyway**, change server retention before you
start: **Settings → Server retention → keep N days** or **never delete**. eeemail
then leaves mail on the server for your other clients to fetch. This is a
supported mode, not a workaround, but decide before the first sync rather than
after it — retention is not retroactive, and mail already deleted from the server
is not coming back.

## It works with everyone else's mail client

Your correspondents do not need eeemail, or Delta Chat, or any knowledge of
encryption. Three cases, all of them working:

- **Ordinary unencrypted email.** Encryption is opportunistic by default: if
  eeemail has no key for a recipient, the message goes out as plain email and
  arrives in Gmail, Outlook or anything else. Nothing is dropped and nothing is
  mangled.
- **Autocrypt.** eeemail advertises an `Autocrypt:` header and reads the ones it
  receives, so two clients that both speak Autocrypt start encrypting without
  either user doing anything.
- **PGP/MIME.** What eeemail sends is standard OpenPGP. It is checked against
  GnuPG on every run of `scripts/gpg-interop-pass.py`, so the outgoing crypto has
  been read by an implementation that is not the one that wrote it, and against
  Delta Chat's own released engine by `scripts/interop-pass.py`.

The incoming direction is the same story: encrypted, signed and plain mail all
arrive, and every message in the reading pane says which it was.

**Two honest caveats.**

- **Keys learned from an `Autocrypt:` header are not authenticated.** Anyone who
  can write the `From` line of a first message can write the `Autocrypt` line.
  That protects you from someone reading stored mail; it does not protect you
  from someone rewriting mail in flight. This is Autocrypt's own threat model,
  and it is why "encrypted" and "verified" are two separate badges — only a QR
  verification survives an active attacker. See
  [ADR 0021](adr/0021-autocrypt-key-contacts.md).
- **A stock Delta Chat will not accept your first message.** Delta Chat ships
  with `force_encryption` on, which refuses to send *or download* cleartext. Your
  first message to one has no key to use yet, so it is dropped before its
  `Autocrypt` header can be read and the exchange never bootstraps. The person on
  the other end has to turn that setting off once. Nothing eeemail can do from
  this side.

**Untested:** Thunderbird, Gmail, Outlook and every mainstream provider. Not
"known broken" — genuinely untested, because it needs credentials and clients
that automated testing here cannot reach. It is the largest gap in the project.

## Where your mail lives

| Platform | Directory |
|---|---|
| Linux | `~/.local/share/eeemail/` (or `$XDG_DATA_HOME/eeemail/`) |
| Windows | `%APPDATA%\eeemail\` |
| macOS | `~/Library/Application Support/eeemail/` |

Accounts are in `accounts/` under that. **Back it up.** The local database is
the mailbox and the server has nothing left to re-download, so losing this
directory loses your mail. If you set a database passphrase in Settings, losing
*that* has the same effect and there is no recovery path — by design.

Setting `EEEMAIL_ACCOUNTS_DIR` points the app at a different accounts directory,
which is how the test scripts run against a scratch profile.

## The three retention deadlines

eeemail deletes mail on a timer in exactly one place, and it is worth knowing
where.

1. **Unverified.** Mail from a sender who is neither verified nor in your
   contacts waits here instead of reaching the inbox. After 30 days — settings:
   never, 7, 30 or 90 — it moves to **Trash**. Accepting or verifying the sender
   before then releases it to the inbox; accepting them *after* does not, because
   by then it is in Trash and yours to restore by hand.
2. **Disappearing messages.** Off by default. If you turn a timer on and it
   fires, the message moves to **Trash** rather than being destroyed. Removal
   from the server and from your correspondent's client is immediate either way.
3. **Trash.** After 30 days — settings: immediately, 7, 30 or 90 — a message in
   Trash is destroyed for real. **This is the only thing in eeemail that destroys
   mail.** Everything else moves it here first.

All three are local and per-device. They deliberately do not sync: a device that
has been offline for six months must not come back and delete mail another
device is still holding.

## Uninstall

```sh
sudo apt remove eeemail          # Debian/Ubuntu
rm eeemail_0.3.0_amd64.AppImage  # AppImage
```

On Windows, use "Add or remove programs".

**None of these delete your mail.** The data directory in the table above is
left alone, so reinstalling picks up where you left off. Delete that directory
by hand if you actually want the mailbox gone — and note that it is the only
copy.

## The command-line tools

The archive holds two programs. Neither is needed to use eeemail, and **the app
does not use them** — it embeds the engine in-process.

**`eeemail-cli`** inspects and configures a mailbox from a shell. It is one-shot:
every invocation opens the account, does one thing, prints JSON, and exits. It
never starts the engine's IO loop, so it **cannot send or receive mail** — it is
a tool for looking at what is already there.

The `<db-path>` is one account's database, not the accounts directory. Each
account lives in a directory named after a UUID, so find it rather than guessing:

```sh
db=$(ls -d ~/.local/share/eeemail/accounts/*/dc.db | head -1)
./eeemail-cli "$db" info
./eeemail-cli "$db" list unverified
./eeemail-cli "$db" show 101
./eeemail-cli "$db" gating days 7
```

Close the app first. Two processes with the same SQLite database open is not a
situation worth being in.

Run it with no arguments for the full command list.

**`deltachat-rpc-server`** speaks JSON-RPC over stdin/stdout and is what
automation drives — the end-to-end and interoperability passes in `scripts/` all
use it. Set `DC_ACCOUNTS_PATH` and talk JSON Lines to it.

## If something goes wrong

Start it from a terminal, where it prints what it is doing:

```sh
eeemail                                    # installed .deb
./eeemail_0.3.0_amd64.AppImage             # AppImage
RUST_LOG=info eeemail                      # with engine logging
```

Then open an [issue](https://github.com/Yjlion/eeemail/issues) with what it
printed. Please do not paste your mail.
