# 0023 — The application says what it is, before it asks for a mail password

**Status:** Accepted — 2026-09-03

## Context

The README opens with a paragraph saying this was largely written by a language
model, is unaudited, and should not be relied on for anything consequential. The
release notes say it again, at more length. Both are true and both are
prominent.

Neither is in the application. Someone who installs a `.deb` and launches it
from a menu has read nothing at all, and the first screen they see asks for the
password to their email account. Every honest sentence this project has written
about itself lives in files that a person who installs it has no particular
reason to open.

There is a second thing that has never been said anywhere the user would see it.
eeemail's transport model ([0003](0003-imap-as-transport.md)) downloads mail and
**removes it from the server**, and the local store is the only durable copy
([0004](0004-local-store-and-raw-mime.md)). Pointing eeemail at an everyday
account therefore drains that account onto one device. The setup form mentioned
the local database in passing; it never said "use a fresh account", which is the
actual advice.

## Decision

**A modal dialog on first launch, before the account list is read and so before
the setup form.** It says four things: this is unaudited development software;
use a dedicated account and why; it still interoperates with ordinary mail
clients; back up the local database, because there is no other copy.

**The third point is not optional padding.** "Use a dedicated account" and
"transport only" read, to a reasonable person, as "this will not work with normal
email" — which is the opposite of true. The dialog says plainly that ordinary
unencrypted mail, Autocrypt and PGP/MIME all reach people who have never heard of
eeemail, because a warning that leaves someone with a false belief is not a
warning that worked.

**Acknowledgement is a file beside the accounts**,
`<data dir>/first-run-acknowledged`, written through a Tauri command. Not a
`Config` value: the disclosure has to appear *before* an account exists, and
`Config` is per-account. Not the account directory either — it outlives any one
account, including deleting them all and starting again, at which point the
person at the keyboard has already read it.

**A `PREVIEW` chip stays in the sidebar** for every non-demo build, on the same
pattern as the existing `DEMO` chip. The dialog is read once and dismissed; the
state it describes lasts considerably longer than that click.

**Native `<dialog>` with `showModal()`**, and `cancel` is suppressed so neither
Escape nor the backdrop dismisses it. There was no modal component anywhere in
`desktop/`, and this is not a reason to acquire one: the platform already
provides the focus trap and the backdrop, and a hand-rolled modal would be new
surface in the process that renders untrusted mail ([0013](0013-desktop-ui.md))
in exchange for nothing.

## Consequences

- **Everyone who launches eeemail has been told what it is.** That was already
  true of everyone who read the README, which is a much smaller group and not
  the one that matters.
- **A dialog is a thing to click past**, and most people will. The `PREVIEW`
  chip is the concession to that: it costs nothing, it does not interrupt, and
  it is still there in a screenshot somebody posts three months later.
- **Failing to write the marker means showing the dialog again**, which is a
  papercut. The command swallows the error rather than refusing to start over
  it, and `first_run_pending` returns `true` when it cannot read the marker —
  both err towards showing a warning twice rather than never.
- **The dialog is screenshotted like every other screen**, through a
  `#/first-run` route that only the demo build honours. This is the same
  discipline `screenshots.sh` applies elsewhere: a change to this text is a
  change to a committed image, reviewable in a diff, rather than something
  nobody looks at again after it is written.
- **It says "a dedicated account is recommended", not "required".** The
  supported alternative — server retention set to *keep N days* or *never* — is
  named in the dialog and explained in [`INSTALL.md`](../INSTALL.md), because
  someone who has decided otherwise deserves the working configuration rather
  than a repeated warning.
- **This is not a substitute for an audit**, and saying so in the application
  does not make the software safer. It makes the person using it better
  informed, which is the only thing available until there is an audit.
