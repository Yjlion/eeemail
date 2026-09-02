/**
 * Contacts, verification and QR.
 *
 * Contacts are not an address book here, they are where the keys live. A
 * contact's verification state is the only claim this client makes about
 * identity that survives an active attacker, so it is the one thing the list
 * shows before anything else.
 */

import { rpc } from "../client";
import { state, changed } from "../state";
import { escapeHtml } from "../html";
import type { Contact } from "../types";

let contacts: Contact[] = [];
let qrSvg: string | null = null;

async function load(): Promise<void> {
  contacts = (await rpc.call("get_contacts", [state.accountId, 0, null])) as Contact[];
  if (qrSvg === null) {
    try {
      const code = (await rpc.call("get_chat_securejoin_qr_code", [
        state.accountId,
        null,
      ])) as string;
      qrSvg = (await rpc.call("create_qr_svg", [code])) as string;
    } catch {
      // An account that is not configured yet has no invite code. The rest of
      // the screen still works, so this is a missing panel, not an error.
      qrSvg = null;
    }
  }
}

export async function renderContacts(el: HTMLElement): Promise<void> {
  await load();
  const verified = contacts.filter((c) => c.isVerified);
  const rest = contacts.filter((c) => !c.isVerified);

  const row = (c: Contact) => `
    <div class="contact" data-contact-id="${c.id}">
      <div>
        <div class="name">${escapeHtml(c.displayName || c.address)}</div>
        <div class="addr">${escapeHtml(c.address)}</div>
      </div>
      <div class="contact-actions">
        ${
          c.isVerified
            ? `<span class="badge verified">verified</span>`
            : `<span class="badge plain">unverified</span>`
        }
        <button data-act="release" data-contact-id="${c.id}">Release held mail</button>
      </div>
    </div>`;

  el.innerHTML = `
    <div class="contacts">
      <h1>Contacts</h1>
      <p class="lede">
        A contact is where a correspondent's key lives. <strong>Verified</strong>
        means you completed a QR exchange with them, which is the only claim here
        that survives someone actively interfering. Anything else means the key
        was learned from mail they sent, which is worth having and is not proof.
      </p>

      <section class="qr-block">
        <h2 class="section">Your invite code</h2>
        <p class="hint">
          Have them scan this, or scan theirs. Either direction verifies both of
          you.
        </p>
        <div class="qr" id="qr">${
          qrSvg ?? `<span class="hint">No invite code yet &mdash; finish setting up your account.</span>`
        }</div>
        <form id="scan-form" class="scan">
          <input name="qr" placeholder="Paste a scanned code" autocomplete="off" />
          <button type="submit">Verify from code</button>
        </form>
        <p class="hint">
          Camera scanning is not wired up: the Linux webview does not reliably
          give a page a camera, and a button that works on one platform and
          silently does nothing on another is worse than no button. Paste or use
          another device meanwhile.
        </p>
      </section>

      <h2 class="section">Verified (${verified.length})</h2>
      ${verified.length ? verified.map(row).join("") : `<div class="empty small">Nobody yet</div>`}
      <h2 class="section">Everyone else (${rest.length})</h2>
      ${rest.length ? rest.map(row).join("") : `<div class="empty small">Nobody yet</div>`}

      <form id="add-contact" class="add-contact">
        <h2 class="section">Add a contact</h2>
        <label>Name <input name="name" autocomplete="off" /></label>
        <label>Address <input name="addr" type="email" required autocomplete="off" /></label>
        <button type="submit">Add</button>
      </form>
      <div class="error" id="contacts-error" hidden></div>
    </div>
  `;

  const error = el.querySelector<HTMLElement>("#contacts-error")!;
  const fail = (err: unknown) => {
    error.hidden = false;
    error.textContent = err instanceof Error ? err.message : String(err);
  };

  el.querySelector<HTMLFormElement>("#scan-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    error.hidden = true;
    const form = event.target as HTMLFormElement;
    const code = (form.elements.namedItem("qr") as HTMLInputElement).value.trim();
    if (!code) return;
    try {
      // Checked before joining: `check_qr` says what the code *is*, and a code
      // that turns out to be an account setup or a group invite must not be run
      // through the contact-verification path by accident.
      const parsed = (await rpc.call("check_qr", [state.accountId, code])) as {
        type: string;
      };
      if (!parsed.type.toLowerCase().includes("verify")) {
        throw new Error(`That code is a ${parsed.type}, not a contact invite.`);
      }
      await rpc.call("secure_join", [state.accountId, code]);
      await renderContacts(el);
      changed();
    } catch (err) {
      fail(err);
    }
  });

  for (const button of el.querySelectorAll<HTMLButtonElement>("button[data-act='release']")) {
    button.addEventListener("click", async () => {
      try {
        const released = (await rpc.call("release_held_contact", [
          state.accountId,
          Number(button.dataset["contactId"]),
        ])) as number;
        error.hidden = false;
        error.className = "notice";
        error.textContent =
          released > 0
            ? `Released ${released} message(s) into the inbox.`
            : `Nothing released: this contact is still neither verified nor in your address book. Accept a message from them, or write to them.`;
      } catch (err) {
        fail(err);
      }
    });
  }

  el.querySelector<HTMLFormElement>("#add-contact")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    error.hidden = true;
    const form = event.target as HTMLFormElement;
    try {
      await rpc.call("create_contact", [
        state.accountId,
        (form.elements.namedItem("addr") as HTMLInputElement).value.trim(),
        (form.elements.namedItem("name") as HTMLInputElement).value.trim() || null,
      ]);
      await renderContacts(el);
    } catch (err) {
      fail(err);
    }
  });
}
