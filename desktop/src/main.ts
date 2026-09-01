/**
 * eeemail's reading UI.
 *
 * Plain DOM, no framework. The surface here is a message list, a threaded
 * reading pane, a label sidebar and search -- the parts of Phase 7 that can be
 * verified against a real account. It deliberately does not paper over the
 * engine: encryption state, undelivered recipients and lost raw MIME are shown
 * as they are, because they are exactly what an encrypted mail client has to be
 * honest about.
 */

import { rpc } from "./rpc";
import { renderPlainText, sandboxedDocument, hasRemoteContent, escapeHtml } from "./html";
import type { Label, MessageCrypto, Recipient, ThreadItem } from "./types";

type View = { kind: "label"; labelId: number; name: string } | { kind: "search"; query: string };

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("missing #app");

let accountId = 0;
let labels: Label[] = [];
let view: View = { kind: "label", labelId: 0, name: "Inbox" };
let messageIds: number[] = [];
let selectedMsgId: number | null = null;

async function boot(): Promise<void> {
  try {
    await rpc.ready();
    const ids = (await rpc.call("get_all_account_ids")) as number[];
    if (ids.length === 0) {
      renderShell();
      showError("No account configured yet. Account setup is not built yet; use eeemail-cli.");
      return;
    }
    accountId = ids[0]!;

    // eeemail's defaults are applied at setup, not as compile-time defaults, so
    // every entry point has to ask for them. See ADR 0012.
    await rpc.call("apply_eeemail_defaults", [accountId]);

    labels = (await rpc.call("get_labels", [accountId])) as Label[];
    renderShell();

    // New mail arrives pushed, not polled.
    rpc.onEvent(({ event }) => {
      const kind = (event as { kind?: string } | undefined)?.kind;
      if (kind === "IncomingMsg" || kind === "MsgsChanged") void refreshList();
    });

    await refreshList();
  } catch (err) {
    renderShell();
    showError(err instanceof Error ? err.message : String(err));
  }
}

function renderShell(): void {
  app!.innerHTML = `
    <nav class="sidebar" id="sidebar"></nav>
    <section class="list-pane" style="display:flex;flex-direction:column;overflow:hidden">
      <div class="search"><input id="search" type="search" placeholder="Search mail" /></div>
      <div class="list" id="list"></div>
    </section>
    <main class="reading" id="reading"><div class="empty">Select a message</div></main>
  `;
  renderSidebar();

  const search = document.querySelector<HTMLInputElement>("#search")!;
  let timer: number | undefined;
  search.addEventListener("input", () => {
    window.clearTimeout(timer);
    // Debounced: search runs a LIKE over the mailbox, and a query per keystroke
    // would issue one for every prefix of a word nobody meant to search for.
    timer = window.setTimeout(() => {
      const query = search.value.trim();
      view = query ? { kind: "search", query } : { kind: "label", labelId: 0, name: "Inbox" };
      renderSidebar();
      void refreshList();
    }, 200);
  });
}

function renderSidebar(): void {
  const sidebar = document.querySelector<HTMLElement>("#sidebar");
  if (!sidebar) return;
  const archive = labels.find((l) => l.isSystem && l.name === "Archive");
  const userLabels = labels.filter((l) => !l.isSystem);

  const button = (id: number, name: string) =>
    `<button data-label-id="${id}" aria-current="${
      view.kind === "label" && view.labelId === id ? "true" : "false"
    }">${escapeHtml(name)}</button>`;

  sidebar.innerHTML = `
    <h2>Mailbox</h2>
    ${button(0, "Inbox")}
    ${archive ? button(archive.id, "Archive") : ""}
    <h2>Labels</h2>
    ${userLabels.length ? userLabels.map((l) => button(l.id, l.name)).join("") : `<div class="empty" style="padding:8px 12px;text-align:left">None yet</div>`}
  `;

  for (const el of sidebar.querySelectorAll<HTMLButtonElement>("button")) {
    el.addEventListener("click", () => {
      const id = Number(el.dataset["labelId"]);
      view = { kind: "label", labelId: id, name: el.textContent ?? "" };
      const search = document.querySelector<HTMLInputElement>("#search");
      if (search) search.value = "";
      renderSidebar();
      void refreshList();
    });
  }
}

async function refreshList(): Promise<void> {
  const list = document.querySelector<HTMLElement>("#list");
  if (!list) return;
  try {
    if (view.kind === "search") {
      messageIds = (await rpc.call("search_email", [accountId, { text: view.query }])) as number[];
    } else if (view.labelId === 0) {
      // The inbox is what has not been archived: archive is the presence of a
      // label, so the inbox needs no rows of its own. See ADR 0009.
      messageIds = (await rpc.call("search_email", [accountId, { archived: false }])) as number[];
    } else {
      messageIds = (await rpc.call("get_label_messages", [accountId, view.labelId])) as number[];
    }
  } catch (err) {
    showError(err instanceof Error ? err.message : String(err));
    return;
  }

  if (messageIds.length === 0) {
    list.innerHTML = `<div class="empty">Nothing here</div>`;
    return;
  }

  const rows = await Promise.all(
    messageIds.slice(0, 200).map(async (id) => {
      const msg = (await rpc.call("get_message", [accountId, id])) as {
        subject?: string;
        text?: string;
      };
      const crypto = (await rpc.call("get_message_crypto", [accountId, id])) as MessageCrypto;
      const subject = msg.subject?.trim() || "(no subject)";
      const preview = (msg.text ?? "").slice(0, 90);
      return `<div class="list-item" data-msg-id="${id}" aria-current="${
        selectedMsgId === id ? "true" : "false"
      }">
        <div class="subject">${escapeHtml(subject)}</div>
        <div class="meta">
          <span>${escapeHtml(preview)}</span>
          ${crypto.encrypted ? `<span class="badge enc">e2e</span>` : `<span class="badge plain">plain</span>`}
        </div>
      </div>`;
    }),
  );
  list.innerHTML = rows.join("");

  for (const el of list.querySelectorAll<HTMLElement>(".list-item")) {
    el.addEventListener("click", () => {
      selectedMsgId = Number(el.dataset["msgId"]);
      for (const other of list.querySelectorAll<HTMLElement>(".list-item")) {
        other.setAttribute("aria-current", other === el ? "true" : "false");
      }
      void renderMessage(selectedMsgId);
    });
  }
}

async function renderMessage(msgId: number): Promise<void> {
  const reading = document.querySelector<HTMLElement>("#reading");
  if (!reading) return;
  try {
    const msg = (await rpc.call("get_message", [accountId, msgId])) as {
      subject?: string;
      text?: string;
      hasHtml?: boolean;
    };
    // HTML mail goes through the sandbox; plain text does not need it.
    const htmlBody = msg.hasHtml
      ? ((await rpc.call("get_message_html", [accountId, msgId])) as string | null)
      : null;
    const recipients = (await rpc.call("get_message_recipients", [accountId, msgId])) as Recipient[];
    const crypto = (await rpc.call("get_message_crypto", [accountId, msgId])) as MessageCrypto;
    const undelivered = (await rpc.call("get_undelivered_recipients", [accountId, msgId])) as string[];
    const retained = (await rpc.call("is_message_raw_mime_retained", [accountId, msgId])) as boolean;
    const threadId = (await rpc.call("get_message_thread", [accountId, msgId])) as number | null;
    const thread =
      threadId === null
        ? []
        : ((await rpc.call("get_thread_tree", [accountId, threadId])) as ThreadItem[]);

    const addrs = (kind: string) =>
      recipients
        .filter((r) => r.kind === kind)
        .map((r) => escapeHtml(r.name ? `${r.name} <${r.addr}>` : r.addr))
        .join(", ");

    const badges = [
      crypto.encrypted
        ? `<span class="badge enc">end-to-end encrypted</span>`
        : `<span class="badge plain">not encrypted</span>`,
      // Only verification survives an active attacker, so it is the only one
      // shown as a positive claim about identity.
      crypto.verified ? `<span class="badge verified">verified contact</span>` : "",
    ].join(" ");

    const body = (htmlBody ?? msg.text ?? "").trim();
    const remote = hasRemoteContent(body);

    reading.innerHTML = `
      <h1>${escapeHtml(msg.subject?.trim() || "(no subject)")}</h1>
      <div class="headers">
        ${addrs("to") ? `<div>To: ${addrs("to")}</div>` : ""}
        ${addrs("cc") ? `<div>Cc: ${addrs("cc")}</div>` : ""}
        ${addrs("bcc") ? `<div>Bcc: ${addrs("bcc")}</div>` : ""}
        <div style="margin-top:6px">${badges}</div>
      </div>
      ${
        undelivered.length
          ? `<div class="notice warn">Not delivered to ${undelivered
              .map(escapeHtml)
              .join(", ")} &mdash; no encryption key was available for them.</div>`
          : ""
      }
      ${
        remote
          ? `<div class="notice warn">Remote content in this message was blocked.</div>`
          : ""
      }
      <div class="body" id="body"></div>
      ${
        thread.length > 1
          ? `<h2 style="font-size:13px;margin-top:24px;color:var(--text-dim)">Conversation</h2>
             <div id="thread"></div>`
          : ""
      }
      <div style="margin-top:24px;font-size:12px;color:var(--text-dim)">
        ${retained ? `Original message source is available.` : `Original source has expired and is no longer available.`}
      </div>
    `;

    const bodyEl = reading.querySelector<HTMLElement>("#body")!;
    if (htmlBody !== null) {
      // Never into the app document: a successful injection here would have the
      // RPC pipe and the whole account within reach.
      renderHtmlBody(bodyEl, htmlBody);
    } else {
      bodyEl.innerHTML = renderPlainText(body);
    }

    if (thread.length > 1) {
      const container = reading.querySelector<HTMLElement>("#thread")!;
      const rows = await Promise.all(
        thread.map(async (item) => {
          const m = (await rpc.call("get_message", [accountId, item.msgId])) as {
            subject?: string;
          };
          return `<div class="list-item ${item.depth > 0 ? "thread-child" : ""}"
                       style="margin-left:${item.depth * 16}px"
                       data-msg-id="${item.msgId}">
            ${escapeHtml(m.subject?.trim() || "(no subject)")}
          </div>`;
        }),
      );
      container.innerHTML = rows.join("");
      for (const el of container.querySelectorAll<HTMLElement>(".list-item")) {
        el.addEventListener("click", () => void renderMessage(Number(el.dataset["msgId"])));
      }
    }
  } catch (err) {
    showError(err instanceof Error ? err.message : String(err));
  }
}

/**
 * Renders HTML mail into a sandboxed frame.
 *
 * This is the only path by which message markup is ever displayed.
 */
function renderHtmlBody(container: HTMLElement, html: string): void {
  const frame = document.createElement("iframe");
  // No `allow-*` tokens: no scripts, no forms, no navigation, `null` origin.
  frame.setAttribute("sandbox", "");
  frame.srcdoc = sandboxedDocument(html);
  container.replaceChildren(frame);
}

function showError(message: string): void {
  const reading = document.querySelector<HTMLElement>("#reading") ?? app!;
  const el = document.createElement("div");
  el.className = "error";
  el.textContent = message;
  reading.prepend(el);
}

void boot();
