/**
 * First-run account setup.
 *
 * `add_transport` and `apply_eeemail_defaults` are called from the *same*
 * function, deliberately. eeemail's defaults are applied at setup rather than
 * as compile-time defaults ([ADR 0012]), so an account configured without that
 * second call is silently left on upstream's strict policy -- and the symptom
 * would be "my mail will not send", days later, with nothing pointing back
 * here. Making them one step is what stops that.
 *
 * [ADR 0012]: ../../../docs/adr/0012-rpc-and-cli.md
 */

import { rpc } from "../client";
import { state, changed } from "../state";

export function renderSetup(el: HTMLElement): void {
  el.innerHTML = `
    <form class="setup" id="setup">
      <h1>Set up your mailbox</h1>
      <p class="lede">
        eeemail uses IMAP and SMTP as transport only. Mail is downloaded,
        decrypted, stored on this device and removed from the server, so the
        local database is your mailbox &mdash; back it up.
      </p>

      <label>Email address <input name="addr" type="email" required autocomplete="username" /></label>
      <label>Password <input name="password" type="password" required autocomplete="current-password" /></label>

      <details>
        <summary>Server settings</summary>
        <p class="hint">Leave blank to autoconfigure from your address.</p>
        <div class="grid">
          <label>IMAP host <input name="imapHost" autocomplete="off" /></label>
          <label>IMAP port <input name="imapPort" type="number" min="1" max="65535" /></label>
          <label>SMTP host <input name="smtpHost" autocomplete="off" /></label>
          <label>SMTP port <input name="smtpPort" type="number" min="1" max="65535" /></label>
        </div>
        <label class="check">
          <input name="acceptInvalidCerts" type="checkbox" />
          Accept an invalid certificate
          <span class="hint">
            Only for a test server with a self-signed certificate. On a real
            account this removes the protection that stops someone else
            answering for your provider.
          </span>
        </label>
      </details>

      <div class="actions">
        <button type="submit">Connect</button>
      </div>
      <div class="notice" id="setup-status" hidden></div>
      <div class="error" id="setup-error" hidden></div>
    </form>
  `;

  const form = el.querySelector<HTMLFormElement>("#setup")!;
  const status = el.querySelector<HTMLElement>("#setup-status")!;
  const error = el.querySelector<HTMLElement>("#setup-error")!;
  const field = (name: string) =>
    (form.elements.namedItem(name) as HTMLInputElement).value.trim();

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    error.hidden = true;
    status.hidden = false;
    status.textContent = "Connecting…";
    const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;
    submit.disabled = true;

    try {
      const accountId =
        state.accountId || ((await rpc.call("add_account")) as number);

      const certs = (form.elements.namedItem("acceptInvalidCerts") as HTMLInputElement)
        .checked
        ? "acceptInvalidCertificates"
        : "automatic";
      await rpc.call("add_transport", [
        accountId,
        {
          addr: field("addr"),
          password: field("password"),
          imapServer: field("imapHost") || null,
          imapPort: Number(field("imapPort")) || null,
          imapUser: null,
          imapSecurity: null,
          certificateChecks: certs,
          smtpServer: field("smtpHost") || null,
          smtpPort: Number(field("smtpPort")) || null,
          smtpUser: null,
          smtpPassword: null,
          smtpSecurity: null,
        },
      ]);

      // Same function as the call above, on purpose. See the module docs.
      await rpc.call("apply_eeemail_defaults", [accountId]);
      await rpc.call("start_io", [accountId]);

      state.accountId = accountId;
      state.screen = null;
      changed();
    } catch (err) {
      status.hidden = true;
      error.hidden = false;
      error.textContent = err instanceof Error ? err.message : String(err);
      submit.disabled = false;
    }
  });
}
