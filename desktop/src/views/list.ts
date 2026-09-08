/**
 * The message list.
 *
 * One RPC for the whole page of rows, not two per row. The reading client used
 * to issue a `get_message` and a `get_message_crypto` per message, which is
 * fine for a demo mailbox and falls over well before a real one.
 */

import { rpc } from "../client";
import { state, changed } from "../state";
import { reload, checkForNewMail } from "../nav";
import { escapeHtml } from "../html";
import { showMenuFor } from "../actions";
import type { MessageRow } from "../types";

/** How many rows are rendered at once. */
const PAGE = 200;

export async function refreshList(): Promise<void> {
  const { accountId, view } = state;
  if (view.kind === "search") {
    state.messageIds = (await rpc.call("search_email", [
      accountId,
      { text: view.query },
    ])) as number[];
  } else if (view.kind === "label") {
    state.messageIds = (await rpc.call("get_label_messages", [
      accountId,
      view.labelId,
    ])) as number[];
  } else {
    state.messageIds = (await rpc.call("get_tagged_messages", [
      accountId,
      view.tag,
    ])) as number[];
  }
}

export function when(timestamp: number): string {
  const date = new Date(timestamp * 1000);
  // Locale-independent and stable: a screenshot regenerated on a machine in
  // another timezone should not produce a different image. Exported so the one
  // rule has one implementation -- a second copy is how two views end up
  // disagreeing about what a timestamp looks like.
  return date.toISOString().slice(0, 16).replace("T", " ");
}

/** What the engine's connectivity value means, said plainly. */
function connectivityLabel(value: number | null): string {
  if (value === null) return "";
  if (value >= 4000) return "Connected";
  if (value >= 3000) return "Getting new messages";
  if (value >= 2000) return "Connecting";
  return "Not connected";
}

/**
 * The strip above the list: search, refresh, and whatever the current view
 * needs.
 *
 * In the list pane rather than the sidebar on purpose. The sidebar is drawn on
 * every route, including the three full-width screens, so a control there would
 * follow the user into the composer and the settings for no reason -- and would
 * change every screenshot rather than the ones it belongs to.
 */
export function renderListHeader(el: HTMLElement): void {
  const inTrash = state.view.kind === "tag" && state.view.tag === "trash";
  const status = connectivityLabel(state.connectivity);

  el.innerHTML = `
    <div class="search">
      <input id="search" type="search" placeholder="Search mail" />
      <button id="refresh" data-act="refresh" ${state.refreshing ? "disabled" : ""}
              title="${status ? escapeHtml(status) : "Check for new mail"}"
              aria-label="Check for new mail">
        ${state.refreshing ? "Checking…" : "Refresh"}
      </button>
    </div>
    ${
      inTrash
        ? `<div class="pane-actions">
             <span class="hint">${state.messageIds.length} message(s) in the trash.</span>
             <button data-act="empty-trash" class="danger"
                     ${state.messageIds.length === 0 ? "disabled" : ""}>Empty trash</button>
           </div>`
        : ""
    }`;

  el.querySelector<HTMLButtonElement>("button[data-act='refresh']")?.addEventListener(
    "click",
    () => void checkForNewMail(),
  );

  el.querySelector<HTMLButtonElement>("button[data-act='empty-trash']")?.addEventListener(
    "click",
    async () => {
      const count = state.messageIds.length;
      // Named count, and what "permanently" costs here specifically. eeemail
      // takes mail off the server, so there is no second copy to fetch again --
      // which is not what "empty trash" means in a client that leaves one.
      const ok = window.confirm(
        `Permanently delete ${count} message(s)?\n\n` +
          "They are removed from this device for good. eeemail keeps mail " +
          "nowhere else, so there is no copy on the server to fetch again.",
      );
      if (!ok) return;
      await rpc.call("empty_trash", [state.accountId]);
      state.selectedMsgId = null;
      await reload();
    },
  );
}

export async function renderList(el: HTMLElement): Promise<void> {
  const ids = state.messageIds.slice(0, PAGE);
  if (ids.length === 0) {
    el.innerHTML = `<div class="empty">${
      state.view.kind === "tag" && state.view.tag === "unverified"
        ? "Nothing waiting. Mail from people you have not accepted appears here."
        : "Nothing here"
    }</div>`;
    return;
  }

  const rows = (await rpc.call("get_message_rows", [state.accountId, ids])) as MessageRow[];

  el.innerHTML = rows
    .map(
      (row) => `
      <div class="list-item${row.unread ? " unread" : ""}" data-msg-id="${row.msgId}"
           aria-current="${state.selectedMsgId === row.msgId ? "true" : "false"}">
        <div class="row-top">
          <span class="from">${
            // On outgoing mail the sender is the user, so a Sent list built from
            // `from` is the user's own name on every row. Who it went to is the
            // only thing that tells the rows apart.
            row.outgoing
              ? `To: ${escapeHtml(row.to || "(no recipient)")}`
              : escapeHtml(row.from || "(unknown sender)")
          }</span>
          <span class="when">${when(row.timestamp)}</span>
        </div>
        <div class="subject">${escapeHtml(row.subject.trim() || "(no subject)")}</div>
        <div class="meta">
          <span class="preview">${escapeHtml(row.preview)}</span>
        </div>
        <div class="badges">
          ${
            row.encrypted
              ? `<span class="badge enc">e2e</span>`
              : `<span class="badge plain">plain</span>`
          }
          ${row.verified ? `<span class="badge verified">verified</span>` : ""}
          ${row.hasAttachment ? `<span class="badge">attachment</span>` : ""}
          ${
            row.tags.includes("unverified")
              ? `<span class="badge warn-badge">unverified</span>`
              : ""
          }
          ${row.tags.includes("trash") ? `<span class="badge warn-badge">trash</span>` : ""}
        </div>
      </div>`,
    )
    .join("");

  const byId = new Map(rows.map((row) => [row.msgId, row]));
  for (const item of el.querySelectorAll<HTMLElement>(".list-item")) {
    const msgId = Number(item.dataset["msgId"]);
    item.addEventListener("click", () => {
      state.selectedMsgId = msgId;
      changed();
    });
    item.addEventListener("contextmenu", (event) => {
      // Selected first, so the reading pane is showing the message the menu is
      // about. A menu acting on something the user cannot see is how the wrong
      // message gets deleted.
      state.selectedMsgId = msgId;
      changed();
      void showMenuFor(event, { msgId, tags: byId.get(msgId)?.tags ?? [] });
    });
  }
}
