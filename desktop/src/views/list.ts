/**
 * The message list.
 *
 * One RPC for the whole page of rows, not two per row. The reading client used
 * to issue a `get_message` and a `get_message_crypto` per message, which is
 * fine for a demo mailbox and falls over well before a real one.
 */

import { rpc } from "../client";
import { state, changed } from "../state";
import { escapeHtml } from "../html";
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

function when(timestamp: number): string {
  const date = new Date(timestamp * 1000);
  // Locale-independent and stable: a screenshot regenerated on a machine in
  // another timezone should not produce a different image.
  return date.toISOString().slice(0, 16).replace("T", " ");
}

export async function renderList(el: HTMLElement): Promise<void> {
  const ids = state.messageIds.slice(0, PAGE);
  if (ids.length === 0) {
    el.innerHTML = `<div class="empty">${
      state.view.kind === "tag" && state.view.tag === "holding"
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
          <span class="from">${escapeHtml(row.from || "(unknown sender)")}</span>
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
            row.tags.includes("holding")
              ? `<span class="badge warn-badge">holding</span>`
              : ""
          }
          ${row.tags.includes("trash") ? `<span class="badge warn-badge">trash</span>` : ""}
        </div>
      </div>`,
    )
    .join("");

  for (const item of el.querySelectorAll<HTMLElement>(".list-item")) {
    item.addEventListener("click", () => {
      state.selectedMsgId = Number(item.dataset["msgId"]);
      changed();
    });
  }
}
