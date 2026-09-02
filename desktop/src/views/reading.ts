/**
 * The reading pane.
 *
 * Deliberately does not paper over the engine: encryption state, undelivered
 * recipients, holding, expiry and lost raw MIME are shown as they are, because
 * they are exactly what an encrypted mail client has to be honest about.
 *
 * Message HTML renders only inside a sandboxed frame, never in the app
 * document. See `docs/adr/0013-desktop-ui.md`.
 */

import { rpc } from "../client";
import { state, changed } from "../state";
import { renderPlainText, sandboxedDocument, hasRemoteContent, escapeHtml } from "../html";
import { TAG_LABELS } from "../types";
import type {
  Message,
  MessageCrypto,
  MessageTags,
  Recipient,
  StructuredObject,
  ThreadItem,
  TrashedMessage,
} from "../types";
import { refreshList } from "./list";
import { refreshHeldCount } from "./sidebar";

function days(from: number, to: number): number {
  return Math.max(0, Math.round((to - from) / 86_400));
}

export async function renderReading(el: HTMLElement): Promise<void> {
  const msgId = state.selectedMsgId;
  if (msgId === null) {
    el.innerHTML = `<div class="empty">Select a message</div>`;
    return;
  }
  const account = state.accountId;

  const [
    msg,
    recipients,
    crypto,
    undelivered,
    retained,
    tags,
    trashed,
    expiresAt,
    threadId,
    structured,
  ] = (await Promise.all([
      rpc.call("get_message", [account, msgId]),
      rpc.call("get_message_recipients", [account, msgId]),
      rpc.call("get_message_crypto", [account, msgId]),
      rpc.call("get_undelivered_recipients", [account, msgId]),
      rpc.call("is_message_raw_mime_retained", [account, msgId]),
      rpc.call("get_message_tags", [account, msgId]),
      rpc.call("get_trashed_message", [account, msgId]),
      rpc.call("get_message_ephemeral_timer", [account, msgId]),
      rpc.call("get_message_thread", [account, msgId]),
      rpc.call("get_structured_data", [account, msgId]),
    ])) as [
      Message,
      Recipient[],
      MessageCrypto,
      string[],
      boolean,
      MessageTags,
      TrashedMessage | null,
      number | null,
      number | null,
      StructuredObject[],
    ];

  const htmlBody = msg.hasHtml
    ? ((await rpc.call("get_message_html", [account, msgId])) as string | null)
    : null;
  const thread =
    threadId === null
      ? []
      : ((await rpc.call("get_thread_tree", [account, threadId])) as ThreadItem[]);

  const addrs = (kind: string) =>
    recipients
      .filter((r) => r.kind === kind)
      .map((r) => escapeHtml(r.name ? `${r.name} <${r.addr}>` : r.addr))
      .join(", ");

  const held = tags.system.includes("holding");
  const now = Math.floor(Date.now() / 1000);

  const notices: string[] = [];
  if (held) {
    notices.push(
      `<div class="notice warn">
         <strong>This sender is not verified and not in your contacts.</strong>
         It is waiting here rather than in your inbox, and will be discarded if
         you do nothing. <button class="inline" data-act="accept">Accept sender</button>
       </div>`,
    );
  }
  if (trashed) {
    notices.push(
      `<div class="notice warn">
         ${
           trashed.reason === "expired"
             ? "This message's timer expired."
             : "You moved this message to the trash."
         }
         It is still here for ${days(now, trashed.purgeAt)} more days.
         <button class="inline" data-act="restore">Restore</button>
       </div>`,
    );
  }
  if (undelivered.length) {
    notices.push(
      `<div class="notice warn">Not delivered to ${undelivered
        .map(escapeHtml)
        .join(", ")} &mdash; no encryption key was available for them.</div>`,
    );
  }

  const body = (htmlBody ?? msg.text ?? "").trim();
  if (hasRemoteContent(body)) {
    notices.push(`<div class="notice warn">Remote content in this message was blocked.</div>`);
  }

  const badges = [
    crypto.encrypted
      ? `<span class="badge enc">end-to-end encrypted</span>`
      : `<span class="badge plain">not encrypted</span>`,
    // Only verification survives an active attacker, so it is the only one
    // shown as a positive claim about identity.
    crypto.verified ? `<span class="badge verified">verified contact</span>` : "",
    ...tags.system.map((t) => `<span class="badge">${TAG_LABELS[t]}</span>`),
    ...tags.user.map((l) => `<span class="badge">${escapeHtml(l.name)}</span>`),
  ]
    .filter(Boolean)
    .join(" ");

  el.innerHTML = `
    <div class="actions">
      <button data-act="reply">Reply</button>
      <button data-act="reply-all">Reply all</button>
      <button data-act="archive">Archive</button>
      <button data-act="trash">Trash</button>
      <label class="timer">
        Expire in
        <select data-act="timer">
          <option value="0"${expiresAt === null ? " selected" : ""}>never</option>
          <option value="3600">1 hour</option>
          <option value="86400">1 day</option>
          <option value="604800">1 week</option>
          <option value="2592000">30 days</option>
          <option value="31536000">1 year</option>
        </select>
      </label>
    </div>
    <h1>${escapeHtml(msg.subject?.trim() || "(no subject)")}</h1>
    <div class="headers">
      ${addrs("to") ? `<div>To: ${addrs("to")}</div>` : ""}
      ${addrs("cc") ? `<div>Cc: ${addrs("cc")}</div>` : ""}
      ${addrs("bcc") ? `<div>Bcc: ${addrs("bcc")}</div>` : ""}
      <div class="badge-row">${badges}</div>
    </div>
    ${notices.join("")}
    <div class="body" id="body"></div>
    ${
      structured.length
        ? `<h2 class="section">What this message says it is</h2><div id="structured"></div>`
        : ""
    }
    ${
      thread.length > 1
        ? `<h2 class="section">Conversation</h2><div id="thread"></div>`
        : ""
    }
    <div class="footnote">
      ${
        retained
          ? "Original message source is available."
          : "Original source has expired and is no longer available."
      }
      ${
        expiresAt !== null
          ? ` This message expires in ${days(now, expiresAt)} days.`
          : ""
      }
    </div>
  `;

  const bodyEl = el.querySelector<HTMLElement>("#body")!;
  if (htmlBody !== null) {
    // Never into the app document: a successful injection here would have the
    // RPC pipe and the whole account within reach.
    const frame = document.createElement("iframe");
    frame.setAttribute("sandbox", "");
    frame.srcdoc = sandboxedDocument(htmlBody);
    bodyEl.replaceChildren(frame);
  } else {
    bodyEl.innerHTML = renderPlainText(body);
  }

  if (structured.length) {
    renderStructured(el.querySelector<HTMLElement>("#structured")!, structured);
  }

  if (thread.length > 1) {
    await renderThread(el.querySelector<HTMLElement>("#thread")!, thread);
  }

  wireActions(el, msgId, msg, recipients);
}

/**
 * Renders what a message claims about itself.
 *
 * Two rules, and the second is why this function exists rather than a template
 * literal at the call site:
 *
 * 1. **Every key and every value is escaped.** This is app-document DOM, not
 *    the sandboxed body frame, so an unescaped value here is an injection into
 *    the process holding the RPC pipe.
 * 2. **Nothing initiates a request, in either branch.** No anchors, no buttons,
 *    no `src`. A URL in structured data renders as text, the same rule
 *    `renderPlainText` follows for links in a body. Trusted data is *allowed*
 *    to drive affordances by ADR 0016; none ships here, because the shell has
 *    no mediated way to open anything yet and inventing one inside this phase
 *    would ship the phishing primitive the ADR exists to contain.
 *
 * What trust changes is presentation, not capability: a card with its type as
 * a heading, versus flat fields behind a notice saying why they are inert.
 */
function renderStructured(container: HTMLElement, objects: StructuredObject[]): void {
  container.innerHTML = objects
    .map((object) => {
      let parsed: unknown;
      try {
        parsed = JSON.parse(object.json);
      } catch {
        // The engine stores only what parsed at receive, so this is
        // unreachable in practice -- and silently dropping the panel is still
        // better than rendering a broken one.
        return "";
      }
      const fields = flattenStructured(parsed);
      const type = typeOf(parsed);
      const rows = fields
        .map(
          ([key, value]) =>
            `<div class="sd-row"><span class="sd-key">${escapeHtml(key)}</span>` +
            `<span class="sd-value">${escapeHtml(value)}</span></div>`,
        )
        .join("");
      if (!object.trusted) {
        return `<div class="notice">
            This is what the sender's software says the message is about. It is
            shown as text only, because the message is not encrypted and signed
            by someone you have accepted.
          </div>
          <div class="sd-inert">${rows}</div>`;
      }
      return `<div class="card">
          <div class="sd-type">${escapeHtml(type)}</div>
          ${rows}
        </div>`;
    })
    .join("");
}

/** The Schema.org `@type`, or a neutral label when there is none. */
function typeOf(value: unknown): string {
  const type = (value as Record<string, unknown> | null)?.["@type"];
  // Unknown types are shown as themselves rather than dropped: the vocabulary
  // is large and grows, and a client that renders only what it recognises
  // discards the rest of the message's meaning.
  return typeof type === "string" && type ? humanize(type) : "Structured data";
}

/** `ParcelDelivery` and `trackingNumber` both become readable labels. */
function humanize(key: string): string {
  return key
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/^./, (c) => c.toUpperCase());
}

/**
 * Flattens a JSON-LD object into label/value pairs, depth first.
 *
 * Nested objects are prefixed rather than nested visually: the shapes senders
 * use are arbitrary, and a renderer that tried to lay them out would be
 * guessing at a structure it does not control.
 */
function flattenStructured(value: unknown, prefix = "", depth = 0): [string, string][] {
  if (depth > 4 || value === null || typeof value !== "object") {
    return prefix ? [[prefix, String(value ?? "")]] : [];
  }
  const out: [string, string][] = [];
  for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
    // JSON-LD plumbing, not something the user asked to see.
    if (key === "@context" || key === "@type") continue;
    const label = prefix ? `${prefix} · ${humanize(key)}` : humanize(key);
    if (child !== null && typeof child === "object") {
      out.push(...flattenStructured(child, label, depth + 1));
    } else {
      out.push([label, String(child ?? "")]);
    }
  }
  return out;
}

async function renderThread(container: HTMLElement, thread: ThreadItem[]): Promise<void> {
  const rows = (await rpc.call("get_message_rows", [
    state.accountId,
    thread.map((t) => t.msgId),
  ])) as { msgId: number; subject: string; from: string }[];
  const byId = new Map(rows.map((r) => [r.msgId, r]));

  container.innerHTML = thread
    .map((item) => {
      const row = byId.get(item.msgId);
      // Indented with padding, not margin: a `.list-item` is full width, so a
      // margin pushes a deeply nested reply off the right edge of the pane.
      return `<div class="list-item ${item.depth > 0 ? "thread-child" : ""}"
                   style="padding-left:${14 + item.depth * 16}px" data-msg-id="${item.msgId}">
        <span class="from">${escapeHtml(row?.from ?? "")}</span>
        ${escapeHtml(row?.subject.trim() || "(no subject)")}
      </div>`;
    })
    .join("");

  for (const el of container.querySelectorAll<HTMLElement>(".list-item")) {
    el.addEventListener("click", () => {
      state.selectedMsgId = Number(el.dataset["msgId"]);
      changed();
    });
  }
}

function wireActions(
  el: HTMLElement,
  msgId: number,
  msg: Message,
  recipients: Recipient[],
): void {
  const account = state.accountId;

  const openComposer = (all: boolean) => {
    const to = recipients.filter((r) => r.kind === "to").map((r) => r.addr);
    const cc = all ? recipients.filter((r) => r.kind === "cc").map((r) => r.addr) : [];
    const subject = msg.subject?.trim() ?? "";
    state.composerDraft = {
      // Reply goes to whoever the message came from; reply-all keeps the copies.
      to: to.join(", "),
      cc: cc.join(", "),
      bcc: "",
      subject: subject.toLowerCase().startsWith("re:") ? subject : `Re: ${subject}`,
      body: `\n\n> ${(msg.text ?? "").split("\n").join("\n> ")}`,
    };
    state.screen = "composer";
    changed();
  };

  const after = async () => {
    await refreshList();
    await refreshHeldCount();
    state.selectedMsgId = null;
    changed();
  };

  for (const button of el.querySelectorAll<HTMLButtonElement>("button[data-act]")) {
    button.addEventListener("click", async () => {
      switch (button.dataset["act"]) {
        case "reply":
          return openComposer(false);
        case "reply-all":
          return openComposer(true);
        case "archive":
          await rpc.call("archive_messages", [account, [msgId]]);
          return after();
        case "trash":
          await rpc.call("trash_messages", [account, [msgId]]);
          return after();
        case "restore":
          await rpc.call("restore_messages", [account, [msgId]]);
          return after();
        case "accept": {
          // Accepting the sender, not the message: past and future mail from
          // them leaves the holding view together.
          const chatId = msg.chatId;
          await rpc.call("accept_chat", [account, chatId]);
          return after();
        }
      }
    });
  }

  const timer = el.querySelector<HTMLSelectElement>("select[data-act='timer']");
  timer?.addEventListener("change", async () => {
    await rpc.call("set_message_ephemeral_timer", [account, msgId, Number(timer.value)]);
    changed();
  });
}
