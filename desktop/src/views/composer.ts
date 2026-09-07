/**
 * The composer.
 *
 * One `send_email` call rather than four separate ones, because the order of
 * those four matters: the recipient set has to reach the database before the
 * message is sent, or every Bcc is silently dropped. Putting the sequence in
 * the engine is what stops a UI getting it wrong.
 * See `docs/adr/0014-recipient-sets-on-the-wire.md`.
 *
 * Formatting is a mode, not a second composer. Both modes produce a plain-text
 * body; formatted mode additionally produces the `text/html` alternative, from
 * `richtext.ts` rather than from whatever the browser's editor left in the DOM.
 * See `docs/adr/0025-composed-html.md`.
 */

import { rpc } from "../client";
import { state, changed } from "../state";
import { reload } from "../nav";
import { escapeHtml } from "../html";
import { stageAttachment } from "../shell";
import { compose, fromText, normalizeHref } from "../richtext";
import type { RecipientSet } from "../types";

/** Splits a comma-separated address field, dropping empties. */
function addresses(value: string): string[] {
  return value
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

/** The formatting the toolbar offers, and the command each one runs. */
const TOOLS: { label: string; title: string; command: string; value?: string }[] = [
  { label: "B", title: "Bold", command: "bold" },
  { label: "I", title: "Italic", command: "italic" },
  { label: "U", title: "Underline", command: "underline" },
  { label: "S", title: "Strikethrough", command: "strikeThrough" },
  { label: "H", title: "Heading", command: "formatBlock", value: "h2" },
  { label: "“ ”", title: "Quote", command: "formatBlock", value: "blockquote" },
  { label: "• List", title: "Bulleted list", command: "insertUnorderedList" },
  { label: "1. List", title: "Numbered list", command: "insertOrderedList" },
  { label: "Code", title: "Code", command: "formatBlock", value: "pre" },
];

export function renderComposer(el: HTMLElement): void {
  const draft = state.composerDraft ?? {
    to: "",
    cc: "",
    bcc: "",
    subject: "",
    body: "",
    html: null as string | null,
  };
  let formatted = draft.html !== null;

  el.innerHTML = `
    <form class="composer" id="composer">
      <h1>New message</h1>
      <label>To <input name="to" value="${escapeHtml(draft.to)}" autocomplete="off" required /></label>
      <label>Cc <input name="cc" value="${escapeHtml(draft.cc)}" autocomplete="off" /></label>
      <label>Bcc <input name="bcc" value="${escapeHtml(draft.bcc)}" autocomplete="off" /></label>
      <label>Subject <input name="subject" value="${escapeHtml(draft.subject)}" autocomplete="off" /></label>
      <label class="attach">Attachment
        <input name="attachment" type="file" />
        <span class="hint">One file per message: the engine carries one, and
        pretending otherwise here would move the surprise further from where you
        chose it.</span>
      </label>
      <div class="format-row">
        <label class="format-toggle">
          <input type="checkbox" id="formatted" ${formatted ? "checked" : ""} />
          Formatted
        </label>
        <div class="toolbar" id="toolbar" ${formatted ? "" : "hidden"}>
          ${TOOLS.map(
            (tool) =>
              `<button type="button" class="tool" data-command="${tool.command}"
                       data-value="${tool.value ?? ""}"
                       title="${escapeHtml(tool.title)}">${escapeHtml(tool.label)}</button>`,
          ).join("")}
          <button type="button" class="tool" data-command="link" title="Link">Link</button>
        </div>
      </div>
      <textarea name="body" rows="16" placeholder="Write your message"
                ${formatted ? "hidden" : ""}>${escapeHtml(draft.body)}</textarea>
      <div class="rich" id="rich" contenteditable="true" role="textbox" aria-multiline="true"
           ${formatted ? "" : "hidden"}></div>
      <div class="notice" id="crypto-note">Checking who has a key…</div>
      <div class="actions">
        <button type="submit">Send</button>
        <button type="button" data-act="cancel">Discard</button>
      </div>
      <div class="error" id="composer-error" hidden></div>
    </form>
  `;

  const form = el.querySelector<HTMLFormElement>("#composer")!;
  const note = el.querySelector<HTMLElement>("#crypto-note")!;
  const error = el.querySelector<HTMLElement>("#composer-error")!;
  const toolbar = el.querySelector<HTMLElement>("#toolbar")!;
  const rich = el.querySelector<HTMLElement>("#rich")!;
  const plain = form.elements.namedItem("body") as HTMLTextAreaElement;
  const toggle = el.querySelector<HTMLInputElement>("#formatted")!;

  // Set as markup, not as a template value: this is the one place in the app
  // that deliberately puts HTML into the app document, and it is HTML we
  // produced ourselves from the user's own draft, never from a message.
  if (draft.html !== null) rich.innerHTML = draft.html;

  const readSet = (): RecipientSet => ({
    to: addresses((form.elements.namedItem("to") as HTMLInputElement).value),
    cc: addresses((form.elements.namedItem("cc") as HTMLInputElement).value),
    bcc: addresses((form.elements.namedItem("bcc") as HTMLInputElement).value),
  });

  // Whether this will go out encrypted is the single most important thing about
  // a message, and the one thing the user cannot see once it is gone. So it is
  // computed while they type rather than reported afterwards.
  const updateCryptoNote = async () => {
    const set = readSet();
    const all = [...set.to, ...set.cc, ...set.bcc];
    if (all.length === 0) {
      note.className = "notice";
      note.textContent = "Add a recipient.";
      return;
    }
    try {
      const contacts = (await rpc.call("get_contacts", [
        state.accountId,
        0,
        null,
      ])) as { address: string; isVerified: boolean }[];
      const known = new Set(contacts.map((c) => c.address.toLowerCase()));
      const missing = all.filter((a) => !known.has(bareAddress(a).toLowerCase()));
      if (missing.length === 0) {
        note.className = "notice good";
        note.textContent = "Everyone here has a key. This will be end-to-end encrypted.";
      } else {
        note.className = "notice warn";
        note.textContent =
          `No key yet for ${missing.join(", ")}. Under the default opportunistic ` +
          `policy this message goes out unencrypted to them, and they are told so.`;
      }
    } catch {
      note.className = "notice";
      note.textContent = "";
    }
  };

  for (const field of ["to", "cc", "bcc"]) {
    (form.elements.namedItem(field) as HTMLInputElement).addEventListener(
      "input",
      () => void updateCryptoNote(),
    );
  }
  void updateCryptoNote();

  toggle.addEventListener("change", () => {
    formatted = toggle.checked;
    if (formatted) {
      // Carrying the text across rather than starting empty. Switching mode is
      // a decision about presentation, not about discarding what was written.
      rich.innerHTML = fromText(plain.value);
    } else {
      plain.value = compose(rich).text;
    }
    toolbar.hidden = !formatted;
    rich.hidden = !formatted;
    plain.hidden = formatted;
    (formatted ? rich : plain).focus();
  });

  for (const tool of toolbar.querySelectorAll<HTMLButtonElement>("button.tool")) {
    // `mousedown`, not `click`: a click moves focus out of the editor first, and
    // a formatting command with no selection does nothing.
    tool.addEventListener("mousedown", (event) => {
      event.preventDefault();
      const command = tool.dataset["command"] ?? "";
      if (command === "link") {
        const typed = window.prompt("Link address");
        // Normalised here rather than passed through: someone typing
        // `example.com` means a link to it, and an href with no scheme is not a
        // link at all once it leaves this window.
        const href = typed ? normalizeHref(typed) : "";
        if (href) document.execCommand("createLink", false, href);
        return;
      }
      // `execCommand` is deprecated and its output differs between engines,
      // which is exactly why nothing it produces reaches the wire: `richtext.ts`
      // re-emits the body from the DOM as a fixed set of tags on send.
      document.execCommand(command, false, tool.dataset["value"] || undefined);
    });
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    error.hidden = true;
    const set = readSet();
    if (set.to.length === 0) {
      error.hidden = false;
      // Refused rather than guessed at: a message with only Cc or Bcc has no
      // visible addressee and no conversation to belong to.
      error.textContent = "A message needs at least one To: address.";
      return;
    }

    const body = formatted ? compose(rich) : { text: plain.value, html: null };
    const file = (form.elements.namedItem("attachment") as HTMLInputElement).files?.[0];
    const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;
    submit.disabled = true;
    submit.textContent = "Sending…";

    try {
      await rpc.call("send_email", [
        state.accountId,
        set,
        (form.elements.namedItem("subject") as HTMLInputElement).value,
        // The plain-text part, always. `html` is an alternative beside it, so a
        // correspondent whose client shows plain text reads the message.
        body.text,
        // A File in the renderer has no filesystem path, so the shell stages
        // the bytes and hands back one.
        file ? await stageAttachment(file) : null,
        body.html,
      ]);
      state.composerDraft = null;
      state.screen = null;
      state.view = { kind: "tag", tag: "sent" };
      state.selectedMsgId = null;
      // Reloaded, not merely repainted: this switches to a view whose contents
      // have just changed, and the message that was sent is the reason.
      await reload();
    } catch (err) {
      error.hidden = false;
      error.textContent = err instanceof Error ? err.message : String(err);
      submit.disabled = false;
      submit.textContent = "Send";
    }
  });

  form.querySelector<HTMLButtonElement>("button[data-act='cancel']")!.addEventListener(
    "click",
    () => {
      state.composerDraft = null;
      state.screen = null;
      changed();
    },
  );
}

function bareAddress(input: string): string {
  const match = /<([^>]+)>/.exec(input);
  return (match?.[1] ?? input).trim();
}
