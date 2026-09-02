/**
 * The sidebar: system tags first, then the user's own.
 *
 * The order is [`SYSTEM_TAGS`], which puts Inbox and Holding at the top because
 * they are the two views with mail waiting in them, and Trash last because it
 * is where things go to stop mattering. See `docs/adr/0017-system-tags.md`.
 */

import { rpc } from "../client";
import { state, changed } from "../state";
import { escapeHtml } from "../html";
import { SYSTEM_TAGS, TAG_LABELS, type SystemTag } from "../types";

export async function refreshHeldCount(): Promise<void> {
  try {
    const ids = (await rpc.call("get_tagged_messages", [state.accountId, "holding"])) as number[];
    state.heldCount = ids.length;
  } catch {
    // A count is decoration. Failing to fetch it must not take the sidebar down.
    state.heldCount = 0;
  }
}

export function renderSidebar(el: HTMLElement): void {
  const current = (test: boolean) => (test ? "true" : "false");

  const tagButton = (tag: SystemTag) => {
    const active = state.view.kind === "tag" && state.view.tag === tag;
    // Only Holding gets a count. Every other view is either mail the user has
    // seen or mail they put somewhere on purpose; Holding is the one that
    // accumulates without them asking, so it is the one worth surfacing.
    const badge =
      tag === "holding" && state.heldCount > 0
        ? `<span class="count">${state.heldCount}</span>`
        : "";
    return `<button data-tag="${tag}" aria-current="${current(active)}">${TAG_LABELS[tag]}${badge}</button>`;
  };

  const userLabels = state.labels.filter((l) => !l.isSystem);
  const labelButton = (id: number, name: string, color: string | null) => {
    const active = state.view.kind === "label" && state.view.labelId === id;
    const dot = color ? `<span class="dot" style="background:${escapeHtml(color)}"></span>` : "";
    return `<button data-label-id="${id}" aria-current="${current(active)}">${dot}${escapeHtml(name)}</button>`;
  };

  el.innerHTML = `
    <div class="brand">eeemail</div>
    <button class="compose" id="compose-btn">Compose</button>
    <h2>Mailbox</h2>
    ${SYSTEM_TAGS.map(tagButton).join("")}
    <h2>Tags</h2>
    ${
      userLabels.length
        ? userLabels.map((l) => labelButton(l.id, l.name, l.color)).join("")
        : `<div class="empty small">None yet</div>`
    }
    <h2>Account</h2>
    <button data-screen="contacts">Contacts</button>
    <button data-screen="settings">Settings</button>
  `;

  for (const button of el.querySelectorAll<HTMLButtonElement>("button")) {
    button.addEventListener("click", () => {
      const { tag, labelId, screen } = button.dataset;
      if (button.id === "compose-btn") {
        state.composerDraft = null;
        state.screen = "composer";
      } else if (screen) {
        state.screen = screen as typeof state.screen;
      } else if (tag) {
        state.screen = null;
        state.view = { kind: "tag", tag: tag as SystemTag };
        state.selectedMsgId = null;
      } else if (labelId) {
        state.screen = null;
        state.view = {
          kind: "label",
          labelId: Number(labelId),
          name: button.textContent ?? "",
        };
        state.selectedMsgId = null;
      }
      changed();
    });
  }
}
