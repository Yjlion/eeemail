/**
 * Settings.
 *
 * The at-rest panel renders `protection().summary` **verbatim**. Encrypting the
 * database leaves attachments and the original source of every retained message
 * in cleartext in the blobdir, so a screen that reported only
 * `databaseEncrypted` would be telling the user something untrue. The engine
 * composes the honest sentence; this shows it.
 * See `docs/adr/0015-at-rest-and-backup.md`.
 */

import { rpc } from "../client";
import { state, changed } from "../state";
import { escapeHtml } from "../html";
import type { AtRestProtection, EncryptionMode, MdnPolicy } from "../types";

export async function renderSettings(el: HTMLElement): Promise<void> {
  const account = state.accountId;
  const [protection, encryption, mdn, gating, holdDays, purgeDays, ephemeral, blobs] =
    (await Promise.all([
      rpc.call("get_at_rest_protection", [account]),
      rpc.call("get_encryption_mode", [account]),
      rpc.call("get_mdn_policy", [account]),
      rpc.call("get_inbox_gating", [account]),
      rpc.call("get_unverified_trash_days", [account]),
      rpc.call("get_trash_purge_days", [account]),
      rpc.call("get_ephemeral_default", [account]),
      rpc.call("get_blob_encryption", [account]),
    ])) as [
      AtRestProtection,
      EncryptionMode,
      MdnPolicy,
      boolean,
      number,
      number,
      number,
      boolean,
    ];

  const option = (value: string, label: string, current: string) =>
    `<option value="${value}"${value === current ? " selected" : ""}>${label}</option>`;

  el.innerHTML = `
    <div class="settings">
      <h1>Settings</h1>

      <section>
        <h2 class="section">Encryption at rest</h2>
        <div class="notice ${protection.partial ? "warn" : protection.databaseEncrypted ? "good" : ""}">
          ${escapeHtml(protection.summary)}
        </div>
        <dl class="facts">
          <dt>Database</dt><dd>${protection.databaseEncrypted ? "encrypted" : "not encrypted"}</dd>
          <dt>Attachments and message sources</dt>
          <dd>${protection.blobsEncrypted ? "encrypted" : "cleartext"}</dd>
        </dl>

        <form id="passphrase-form" class="inline-form">
          <label>Database passphrase
            <input name="passphrase" type="password" autocomplete="new-password"
                   placeholder="${protection.databaseEncrypted ? "change it" : "set one"}" />
          </label>
          <button type="submit">${protection.databaseEncrypted ? "Change" : "Set"}</button>
        </form>
        <p class="hint">
          There is no recovery. If you lose this, the mailbox is gone &mdash; the
          server is a spool with nothing left to re-download. An empty value
          turns database encryption off.
        </p>

        <label class="check">
          <input type="checkbox" data-set="blobs" ${blobs ? "checked" : ""}
                 ${protection.databaseEncrypted ? "" : "disabled"} />
          Also encrypt attachments and message sources on disk
        </label>
        <p class="hint">
          ${
            protection.databaseEncrypted
              ? `Turning this on rewrites everything already in the blobdir, which
                 takes a while on a large mailbox and can be interrupted safely.
                 Turning it off rewrites it back.`
              : `Needs a database passphrase first: the key for this lives in the
                 database, so encrypting attachments without encrypting the
                 database would protect nothing.`
          }
        </p>
      </section>

      <section>
        <h2 class="section">Encryption in transit</h2>
        <label>Mode
          <select data-set="encryption">
            ${option("strict", "Strict — refuse to send unencrypted", encryption)}
            ${option("opportunistic", "Opportunistic — encrypt when a key is known", encryption)}
            ${option("lenient", "Lenient — send unencrypted rather than fail", encryption)}
          </select>
        </label>
        <p class="hint">
          Opportunistic is the default. Whichever you pick, every message says
          which it was.
        </p>
      </section>

      <section>
        <h2 class="section">Unverified mail</h2>
        <label class="check">
          <input type="checkbox" data-set="gating" ${gating ? "checked" : ""} />
          Hold mail from senders who are neither verified nor in my contacts
        </label>
        <label>Move it to Trash after
          <select data-set="hold">
            ${option("0", "Never — let it wait indefinitely", String(holdDays))}
            ${option("7", "1 week", String(holdDays))}
            ${option("30", "30 days", String(holdDays))}
            ${option("90", "90 days", String(holdDays))}
          </select>
        </label>
        <p class="hint">
          Held mail waits in <strong>Unverified</strong> until you accept or
          verify the sender. If you never do, it moves to <strong>Trash</strong>
          rather than being destroyed &mdash; but accepting the sender after that
          will not bring it back, so restore it from Trash by hand.
          Turning the hold off releases everything currently waiting.
        </p>
      </section>

      <section>
        <h2 class="section">Trash</h2>
        <label>Delete from Trash after
          <select data-set="purge">
            ${option("0", "Immediately — do not keep a copy", String(purgeDays))}
            ${option("7", "1 week", String(purgeDays))}
            ${option("30", "30 days", String(purgeDays))}
            ${option("90", "90 days", String(purgeDays))}
          </select>
        </label>
        <p class="hint">
          This covers everything in Trash, however it got there: thrown away by
          hand, expired by a timer, or swept out of Unverified. Trash is the only
          place in eeemail that destroys mail, and this is how long it waits
          first. Each message keeps the window it was given when it arrived, so
          changing this does not re-time what is already there.
        </p>
      </section>

      <section>
        <h2 class="section">Disappearing messages</h2>
        <label>Timer for new conversations
          <select data-set="ephemeral">
            ${option("0", "Off", String(ephemeral))}
            ${option("86400", "1 day", String(ephemeral))}
            ${option("604800", "1 week", String(ephemeral))}
            ${option("2592000", "30 days", String(ephemeral))}
            ${option("31536000", "1 year", String(ephemeral))}
          </select>
        </label>
        <p class="hint">
          Off by default: whether your mail expires is your call. When a timer
          fires the message moves to <strong>Trash</strong> and stays readable,
          so you can change your mind &mdash; for as long as the Trash section
          above says. Removal from the server and from the other person's client
          is immediate either way.
        </p>
      </section>

      <section>
        <h2 class="section">Read receipts</h2>
        <label>Send to
          <select data-set="mdn">
            ${option("never", "Nobody", mdn)}
            ${option("verifiedOnly", "Verified contacts in my address book", mdn)}
            ${option("always", "Anyone who asks", mdn)}
          </select>
        </label>
      </section>

      <div class="error" id="settings-error" hidden></div>
    </div>
  `;

  const error = el.querySelector<HTMLElement>("#settings-error")!;
  const guard = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
    } catch (err) {
      // Reset the class as well as the text: the same element is used for the
      // migration's progress notice, so a failure after one would otherwise be
      // shown in the reassuring colour.
      error.hidden = false;
      error.className = "error";
      error.textContent = err instanceof Error ? err.message : String(err);
    }
  };

  el.querySelector<HTMLFormElement>("#passphrase-form")?.addEventListener(
    "submit",
    (event) => {
      event.preventDefault();
      const form = event.target as HTMLFormElement;
      const value = (form.elements.namedItem("passphrase") as HTMLInputElement).value;
      void guard(async () => {
        await rpc.call("set_database_passphrase", [account, value]);
        // Repaint from the engine rather than from what we just asked for:
        // encrypting the database does not encrypt the blobdir, and the summary
        // is the only thing that says so.
        await renderSettings(el);
      });
    },
  );

  el.querySelector<HTMLInputElement>("input[data-set='blobs']")?.addEventListener(
    "change",
    (event) => {
      const on = (event.target as HTMLInputElement).checked;
      const status = el.querySelector<HTMLElement>("#settings-error")!;
      status.hidden = false;
      status.className = "notice";
      status.textContent = on
        ? "Encrypting what is already on disk…"
        : "Decrypting what is on disk…";
      void guard(async () => {
        const converted = (await rpc.call(
          on ? "enable_blob_encryption" : "disable_blob_encryption",
          [account],
        )) as number;
        await renderSettings(el);
        const done = el.querySelector<HTMLElement>("#settings-error")!;
        done.hidden = false;
        done.className = "notice good";
        done.textContent = `Converted ${converted} file(s).`;
      });
    },
  );

  el.querySelector<HTMLSelectElement>("select[data-set='encryption']")?.addEventListener(
    "change",
    (event) =>
      void guard(() =>
        rpc.call("set_encryption_mode", [account, (event.target as HTMLSelectElement).value]),
      ),
  );
  el.querySelector<HTMLSelectElement>("select[data-set='hold']")?.addEventListener(
    "change",
    (event) =>
      void guard(async () => {
        await rpc.call("set_unverified_trash_days", [
          account,
          Number((event.target as HTMLSelectElement).value),
        ]);
        // Repaint, not because anything moved yet -- the sweep runs in
        // housekeeping, not on this click -- but because the deadline is read
        // afresh on every sweep, so shortening this changes what the *next*
        // one takes, including mail already waiting.
        changed();
      }),
  );
  el.querySelector<HTMLSelectElement>("select[data-set='purge']")?.addEventListener(
    "change",
    (event) =>
      void guard(() =>
        rpc.call("set_trash_purge_days", [
          account,
          Number((event.target as HTMLSelectElement).value),
        ]),
      ),
  );
  el.querySelector<HTMLSelectElement>("select[data-set='mdn']")?.addEventListener(
    "change",
    (event) =>
      void guard(() =>
        rpc.call("set_mdn_policy", [account, (event.target as HTMLSelectElement).value]),
      ),
  );
  el.querySelector<HTMLSelectElement>("select[data-set='ephemeral']")?.addEventListener(
    "change",
    (event) =>
      void guard(() =>
        rpc.call("set_ephemeral_default", [
          account,
          Number((event.target as HTMLSelectElement).value),
        ]),
      ),
  );
  el.querySelector<HTMLInputElement>("input[data-set='gating']")?.addEventListener(
    "change",
    (event) =>
      void guard(async () => {
        await rpc.call("set_inbox_gating", [account, (event.target as HTMLInputElement).checked]);
        // Turning gating off releases held mail, so the sidebar count and the
        // inbox both change underneath the user.
        changed();
      }),
  );
}
