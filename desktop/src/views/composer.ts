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
 * `richtext.ts` rather than from whatever the editor left in the DOM. The editor
 * is Squire, and the same filter is what it parses pastes and drafts with.
 * See `docs/adr/0025-composed-html.md` and `docs/adr/0030-the-composer-edits-with-squire.md`.
 */

import { rpc } from "../client";
import { state, changed } from "../state";
import { reload } from "../nav";
import { escapeHtml } from "../html";
import { stageAttachment } from "../shell";
import Squire from "squire-rte";
import {
  ALIGNMENTS,
  COLOURS,
  FONTS,
  HIGHLIGHTS,
  SIZES,
  compose,
  fromText,
  normalizeHref,
  sanitizeToFragment,
} from "../richtext";
import type { RecipientSet } from "../types";

/** Splits a comma-separated address field, dropping empties. */
function addresses(value: string): string[] {
  return value
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

/** Sets every selected paragraph or heading to `tag`, keeping its content and alignment. */
function setBlocks(editor: Squire, tag: string): void {
  // Squire has no heading command. `modifyBlocks` is its way of saying "these
  // blocks, lifted out; give back what goes in their place", with undo intact.
  editor.modifyBlocks((frag) => {
    for (const block of Array.from(frag.querySelectorAll("p, h1, h2, h3"))) {
      const replacement = document.createElement(tag);
      for (const attr of Array.from(block.attributes)) {
        replacement.setAttribute(attr.name, attr.value);
      }
      replacement.append(...Array.from(block.childNodes));
      block.replaceWith(replacement);
    }
    return frag;
  });
  editor.focus();
}

/** Asks for a link address, or removes the link the selection is already in. */
function link(editor: Squire): void {
  if (editor.hasFormat("A")) {
    editor.removeLink();
    return;
  }
  const typed = window.prompt("Link address");
  // Normalised here rather than passed through: someone typing `example.com`
  // means a link to it, and an href with no scheme is not a link at all once it
  // leaves this window.
  const href = typed ? normalizeHref(typed) : "";
  if (href) editor.makeLink(href);
}

/** The toolbar's buttons: what each does, and when it shows as pressed. */
const TOOLS: {
  label: string;
  title: string;
  active: (editor: Squire) => boolean;
  run: (editor: Squire) => void;
}[] = [
  {
    label: "B",
    title: "Bold (Ctrl+B)",
    active: (e) => e.hasFormat("B"),
    run: (e) => (e.hasFormat("B") ? e.removeBold() : e.bold()),
  },
  {
    label: "I",
    title: "Italic (Ctrl+I)",
    active: (e) => e.hasFormat("I"),
    run: (e) => (e.hasFormat("I") ? e.removeItalic() : e.italic()),
  },
  {
    label: "U",
    title: "Underline (Ctrl+U)",
    active: (e) => e.hasFormat("U"),
    run: (e) => (e.hasFormat("U") ? e.removeUnderline() : e.underline()),
  },
  {
    label: "S",
    title: "Strikethrough (Ctrl+Shift+7)",
    active: (e) => e.hasFormat("S"),
    run: (e) => (e.hasFormat("S") ? e.removeStrikethrough() : e.strikethrough()),
  },
  {
    label: "H",
    title: "Heading",
    active: (e) => e.hasFormat("H2"),
    run: (e) => setBlocks(e, e.hasFormat("H2") ? "P" : "H2"),
  },
  {
    label: "“ ”",
    title: "Quote (Ctrl+])",
    active: (e) => e.hasFormat("BLOCKQUOTE"),
    run: (e) => (e.hasFormat("BLOCKQUOTE") ? e.decreaseQuoteLevel() : e.increaseQuoteLevel()),
  },
  {
    label: "• List",
    title: "Bulleted list (Ctrl+Shift+8)",
    active: (e) => e.hasFormat("UL"),
    run: (e) => (e.hasFormat("UL") ? e.removeList() : e.makeUnorderedList()),
  },
  {
    label: "1. List",
    title: "Numbered list (Ctrl+Shift+9)",
    active: (e) => e.hasFormat("OL"),
    run: (e) => (e.hasFormat("OL") ? e.removeList() : e.makeOrderedList()),
  },
  {
    label: "Code",
    title: "Code (Ctrl+D)",
    active: (e) => e.hasFormat("CODE") || e.hasFormat("PRE"),
    run: (e) => e.toggleCode(),
  },
  { label: "Link", title: "Link", active: (e) => e.hasFormat("A"), run: link },
];

/**
 * The toolbar's menus. Their options are the lists `richtext.ts` checks against,
 * so nothing here can offer a style the filter would strip on send.
 */
const MENUS: {
  label: string;
  options: { label: string; value: string }[];
  run: (editor: Squire, value: string | null) => void;
}[] = [
  { label: "Font", options: FONTS, run: (e, v) => e.setFontFace(v) },
  { label: "Size", options: SIZES, run: (e, v) => e.setFontSize(v) },
  { label: "Colour", options: COLOURS, run: (e, v) => e.setTextColor(v) },
  { label: "Highlight", options: HIGHLIGHTS, run: (e, v) => e.setHighlightColor(v) },
  {
    label: "Align",
    options: ALIGNMENTS.map((a) => ({ label: a[0]!.toUpperCase() + a.slice(1), value: a })),
    // An empty alignment is Squire's way of removing one.
    run: (e, v) => e.setTextAlignment(v ?? ""),
  },
];

/**
 * The editor of the composer on screen, if there is one.
 *
 * Kept so it can be destroyed: `paint()` replaces the whole screen on every
 * change, and Squire listens on the document as well as on its own root, so an
 * editor that is merely detached is still attached to something.
 */
let current: Squire | null = null;

function closeEditor(): void {
  current?.destroy();
  current = null;
}

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
      <label>Importance
        <select name="importance">
          <option value="normal" selected>Normal</option>
          <option value="high">High</option>
          <option value="low">Low</option>
        </select>
      </label>
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
            (tool, i) =>
              `<button type="button" class="tool" data-tool="${i}" aria-pressed="false"
                       title="${escapeHtml(tool.title)}">${escapeHtml(tool.label)}</button>`,
          ).join("")}
          ${MENUS.map(
            (menu, i) =>
              `<select class="tool-menu" data-menu="${i}" title="${escapeHtml(menu.label)}">
                 <option value="" hidden selected>${escapeHtml(menu.label)}</option>
                 <option value="default">Default</option>
                 ${menu.options
                   .map(
                     (option) =>
                       `<option value="${escapeHtml(option.value)}">${escapeHtml(option.label)}</option>`,
                   )
                   .join("")}
               </select>`,
          ).join("")}
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

  closeEditor();
  const editor = new Squire(rich, {
    // The editor's blocks in the shape the wire gets them, rather than `<div>`s
    // renamed on the way out.
    blockTag: "P",
    // Squire would otherwise reach for a global DOMPurify, and throw without
    // one. Ours is the filter that decides what is sent, so a paste is shown as
    // it will go out.
    sanitizeToDOMFragment: sanitizeToFragment,
  });
  current = editor;
  // Subscript and superscript are not in the whitelist. A shortcut that
  // formats text and then silently loses the formatting on send is worse than
  // one that does nothing.
  for (const key of ["Ctrl-Shift-5", "Ctrl-Shift-6", "Meta-Shift-5", "Meta-Shift-6"]) {
    editor.setKeyHandler(key, null);
  }

  // This is the one place in the app that deliberately puts HTML into the app
  // document. It is HTML we produced ourselves from the user's own draft, and
  // `setHTML` passes it through the send filter on the way in regardless.
  if (draft.html !== null) editor.setHTML(draft.html);

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
      editor.setHTML(fromText(plain.value));
    } else {
      plain.value = compose(editor.getHTML()).text;
    }
    toolbar.hidden = !formatted;
    rich.hidden = !formatted;
    plain.hidden = formatted;
    if (formatted) editor.focus();
    else plain.focus();
  });

  const buttons = Array.from(toolbar.querySelectorAll<HTMLButtonElement>("button.tool"));
  for (const button of buttons) {
    const tool = TOOLS[Number(button.dataset["tool"])]!;
    // `mousedown`, not `click`: a click moves focus out of the editor first, and
    // the caret the command should act on goes with it.
    button.addEventListener("mousedown", (event) => {
      event.preventDefault();
      tool.run(editor);
    });
  }

  for (const select of toolbar.querySelectorAll<HTMLSelectElement>("select.tool-menu")) {
    const menu = MENUS[Number(select.dataset["menu"])]!;
    // A select takes focus, unlike the buttons. Squire keeps the last selection
    // while it is blurred, so the command still lands on what was selected.
    select.addEventListener("change", () => {
      menu.run(editor, select.value === "default" ? null : select.value);
      // Back to the label, so the menu names what it does rather than what was
      // last picked -- which may not describe the text the caret is in now.
      select.value = "";
      editor.focus();
    });
  }

  // Pressed state follows the caret, so the toolbar says what a click will undo.
  editor.addEventListener("pathChange", () => {
    buttons.forEach((button, i) => {
      button.setAttribute("aria-pressed", String(TOOLS[i]!.active(editor)));
    });
  });

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

    const body = formatted ? compose(editor.getHTML()) : { text: plain.value, html: null };
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
        // Normal puts no header on the message at all, which is what keeps an
        // ordinary message identical to one sent before this control existed.
        (form.elements.namedItem("importance") as HTMLSelectElement).value,
      ]);
      closeEditor();
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
      closeEditor();
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
