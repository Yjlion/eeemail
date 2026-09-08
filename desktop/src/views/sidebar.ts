/**
 * The sidebar: system tags first, then the user's own.
 *
 * The order is [`SYSTEM_TAGS`], which puts Inbox and Unverified at the top
 * because they are the two views with mail waiting in them, and Trash last
 * because it is where things go to stop mattering.
 * See `docs/adr/0017-system-tags.md`.
 *
 * Above all of it is the account switcher, and it is drawn only when there is
 * more than one account: a picker with one entry is a control that cannot do
 * anything, taking the top of the sidebar from the mail.
 */

import { state, changed } from "../state";
import { reload } from "../nav";
import { switchAccount } from "../accounts";
import { escapeHtml } from "../html";
import { SYSTEM_TAGS, TAG_LABELS, accountLabel, type SystemTag } from "../types";

export function renderSidebar(el: HTMLElement): void {
  const current = (test: boolean) => (test ? "true" : "false");

  const tagButton = (tag: SystemTag) => {
    const active = state.view.kind === "tag" && state.view.tag === tag;
    // Only Unverified gets a count. Every other view is either mail the user
    // has seen or mail they put somewhere on purpose; Unverified is the one
    // that accumulates without them asking, so it is the one worth surfacing.
    const badge =
      tag === "unverified" && state.unverifiedCount > 0
        ? `<span class="count">${state.unverifiedCount}</span>`
        : "";
    return `<button data-tag="${tag}" aria-current="${current(active)}">${TAG_LABELS[tag]}${badge}</button>`;
  };

  const userLabels = state.labels.filter((l) => !l.isSystem);
  const labelButton = (id: number, name: string, color: string | null) => {
    const active = state.view.kind === "label" && state.view.labelId === id;
    const dot = color ? `<span class="dot" style="background:${escapeHtml(color)}"></span>` : "";
    return `<button data-label-id="${id}" aria-current="${current(active)}">${dot}${escapeHtml(name)}</button>`;
  };

  // Only when there is a choice to make. One account is not a switcher.
  const accountSwitcher =
    state.accounts.length > 1
      ? `<select class="account-switcher" id="account-switcher" aria-label="Account">
          ${state.accounts
            .map(
              (a) =>
                `<option value="${a.id}"${a.id === state.accountId ? " selected" : ""}>${escapeHtml(
                  accountLabel(a),
                )}</option>`,
            )
            .join("")}
        </select>`
      : "";

  el.innerHTML = `
    <div class="brand">eeemail</div>
    ${accountSwitcher}
    <button class="compose" id="compose-btn">Compose</button>
    <h2>Mailbox</h2>
    ${SYSTEM_TAGS.map(tagButton).join("")}
    <h2>Tags</h2>
    ${
      userLabels.length
        ? userLabels.map((l) => labelButton(l.id, l.name, l.color)).join("")
        : `<div class="empty small">None yet</div>`
    }
    <button class="quiet" data-screen="tags">Manage tags…</button>
    <h2>Account</h2>
    <button data-screen="contacts">Contacts</button>
    <button data-screen="settings">Settings</button>
    <button class="quiet" id="add-account-btn">Add a mailbox…</button>
  `;

  el.querySelector<HTMLSelectElement>("#account-switcher")?.addEventListener(
    "change",
    (event) => {
      void switchAccount(Number((event.target as HTMLSelectElement).value)).then(changed);
    },
  );

  for (const button of el.querySelectorAll<HTMLButtonElement>("button")) {
    button.addEventListener("click", () => {
      const { tag, labelId, screen } = button.dataset;
      if (button.id === "compose-btn") {
        state.composerDraft = null;
        state.screen = "composer";
      } else if (button.id === "add-account-btn") {
        // `null` is the request to create one. The form reads this rather than
        // `accountId`, or "add a mailbox" reconfigures the open mailbox.
        state.setupAccountId = null;
        state.screen = "setup";
      } else if (screen) {
        state.screen = screen as typeof state.screen;
      } else if (tag) {
        state.screen = null;
        state.view = { kind: "tag", tag: tag as SystemTag };
        state.selectedMsgId = null;
        // `reload`, not `changed`. A repaint renders `state.messageIds`, which
        // nothing here has refetched -- so for two releases clicking Sent drew
        // the Sent heading over the inbox's rows. Every view had this; Sent and
        // Trash are only where it was impossible to miss.
        return void reload();
      } else if (labelId) {
        state.screen = null;
        state.view = {
          kind: "label",
          labelId: Number(labelId),
          name: button.textContent ?? "",
        };
        state.selectedMsgId = null;
        return void reload();
      }
      changed();
    });
  }
}
