/**
 * The first-launch disclosure.
 *
 * The README and the release notes have always been candid about what this
 * software is; the running application said nothing, which meant the only
 * people who knew were the ones who had already gone looking. This is that
 * text, in front of the person about to point it at their mail.
 *
 * Two things it has to get across, and they pull in opposite directions:
 *
 * 1. **Do not rely on this yet.** Unaudited, and an encrypted mail client is
 *    exactly the kind of software where that matters.
 * 2. **A dedicated account is recommended** -- because IMAP is transport only
 *    ([ADR 0003]), so mail is downloaded and removed from the server, and
 *    pointing eeemail at an everyday account drains it onto this device.
 *
 * The second could be read as "this will not work with normal email", which is
 * the opposite of true, so it says so: ordinary unencrypted mail, Autocrypt and
 * PGP/MIME all reach people who have never heard of eeemail.
 *
 * Native `<dialog>` with `showModal()`. No dependency, no change to the CSP the
 * `frontend` CI job asserts, and the platform already does the focus trap.
 *
 * [ADR 0003]: ../../../docs/adr/0003-imap-as-transport.md
 */

import { acknowledgeFirstRun } from "../shell";

/**
 * Shows the disclosure and resolves once it has been acknowledged.
 *
 * Resolves rather than returning a value, because there is nothing to decide:
 * the one button means "I have read this". A user who has not read it has not
 * got a way past, which is the point of `showModal` and of swallowing `cancel`.
 */
export function showFirstRun(): Promise<void> {
  return new Promise((resolve) => {
    const dialog = document.createElement("dialog");
    dialog.className = "first-run";
    dialog.innerHTML = `
      <h1>eeemail is not finished</h1>

      <div class="notice warn">
        <strong>This is development software and has not been audited by a
        security professional.</strong> It is reviewed and tested, and most of it
        was written by a large language model under human direction. An encrypted
        mail client is exactly the kind of software where that distinction
        matters. Do not rely on it for anything consequential yet.
      </div>

      <h2>Use a dedicated account</h2>
      <p>
        eeemail uses IMAP and SMTP as <em>transport only</em>. Mail is
        downloaded, decrypted, stored on this device and then removed from the
        server &mdash; so this device becomes your mailbox, and pointing eeemail
        at the account you already read elsewhere will drain it into here.
      </p>
      <p class="hint">
        If you would rather share one account with another client, change
        <strong>server retention</strong> in Settings before you start, so
        eeemail leaves mail on the server for everything else to fetch.
      </p>

      <h2>It still talks to everyone else</h2>
      <p>
        Your correspondents do not need eeemail. Mail to someone whose key you do
        not have goes out as ordinary unencrypted email and arrives in any
        client; where a key is known, eeemail uses standard Autocrypt and
        PGP/MIME that Thunderbird, GnuPG and Delta Chat can read. Every message
        says which it was.
      </p>

      <h2>Back it up</h2>
      <p>
        The local database is the mailbox, and the server is a spool with nothing
        left to re-download. There is no recovery from losing it, and none from
        forgetting the database passphrase if you set one.
      </p>

      <form method="dialog">
        <button value="ok" autofocus>I understand</button>
      </form>
    `;

    // Escape and the backdrop must not dismiss this. `cancel` fires for both,
    // and a dialog nobody can read past by accident is the entire point.
    dialog.addEventListener("cancel", (event) => event.preventDefault());
    dialog.addEventListener("close", () => {
      void acknowledgeFirstRun()
        // Failing to write the marker means seeing this again next launch,
        // which is a papercut. Failing to *start* over it would not be.
        .catch(() => {})
        .then(() => {
          dialog.remove();
          resolve();
        });
    });

    document.body.append(dialog);
    dialog.showModal();
  });
}
