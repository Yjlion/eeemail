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
 *
 * The signature and the padlock are the composer's to decide, not the
 * engine's: the signature is placed in the body where the user can see and
 * edit it, and the padlock says per message whether it goes out encrypted.
 * The engine is told both, and still refuses what the encryption policy does
 * not allow. See `docs/adr/0032-composer-send-options.md`.
 */

import { rpc } from "../client";
import { state, changed, type ComposerDraft } from "../state";
import { reload } from "../nav";
import { escapeHtml } from "../html";
import { stageAttachment } from "../shell";
import Squire from "squire-rte";
import {
  ALIGNMENTS,
  FONTS,
  PALETTE,
  SIZES,
  compose,
  fromText,
  normalizeHref,
  sanitizeToFragment,
} from "../richtext";
import { colourIcon, icon, type IconName } from "../icons";
import { closePopover, onPress, togglePopover } from "./popover";
import type { RecipientSet, SendOptions, SendReadiness } from "../types";

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

/** A toolbar button: what it does, and when it shows as pressed. */
type Tool = {
  icon: IconName;
  title: string;
  active?: (editor: Squire) => boolean;
  run: (editor: Squire) => void;
};

/** A toolbar button that opens a menu rather than acting. */
type Menu = {
  menu: "font" | "size" | "colour" | "align";
  title: string;
};

/**
 * The formatting toolbar, in groups. Every style it can apply is one
 * `richtext.ts` keeps on send, so nothing here formats text and then silently
 * loses the formatting.
 */
const TOOLBAR: (Tool | Menu)[][] = [
  [
    { icon: "undo", title: "Undo (Ctrl+Z)", run: (e) => e.undo() },
    { icon: "redo", title: "Redo (Ctrl+Y)", run: (e) => e.redo() },
  ],
  [
    { menu: "font", title: "Font" },
    { menu: "size", title: "Size" },
  ],
  [
    {
      icon: "bold",
      title: "Bold (Ctrl+B)",
      active: (e) => e.hasFormat("B"),
      run: (e) => (e.hasFormat("B") ? e.removeBold() : e.bold()),
    },
    {
      icon: "italic",
      title: "Italic (Ctrl+I)",
      active: (e) => e.hasFormat("I"),
      run: (e) => (e.hasFormat("I") ? e.removeItalic() : e.italic()),
    },
    {
      icon: "underline",
      title: "Underline (Ctrl+U)",
      active: (e) => e.hasFormat("U"),
      run: (e) => (e.hasFormat("U") ? e.removeUnderline() : e.underline()),
    },
    {
      icon: "strike",
      title: "Strikethrough (Ctrl+Shift+7)",
      active: (e) => e.hasFormat("S"),
      run: (e) => (e.hasFormat("S") ? e.removeStrikethrough() : e.strikethrough()),
    },
    { menu: "colour", title: "Text colour and highlight" },
  ],
  [
    { menu: "align", title: "Align" },
    {
      icon: "list",
      title: "Bulleted list (Ctrl+Shift+8)",
      active: (e) => e.hasFormat("UL"),
      run: (e) => (e.hasFormat("UL") ? e.removeList() : e.makeUnorderedList()),
    },
    {
      icon: "listOrdered",
      title: "Numbered list (Ctrl+Shift+9)",
      active: (e) => e.hasFormat("OL"),
      run: (e) => (e.hasFormat("OL") ? e.removeList() : e.makeOrderedList()),
    },
  ],
  [
    {
      icon: "heading",
      title: "Heading",
      active: (e) => e.hasFormat("H2"),
      run: (e) => setBlocks(e, e.hasFormat("H2") ? "P" : "H2"),
    },
    {
      icon: "quote",
      title: "Quote (Ctrl+])",
      active: (e) => e.hasFormat("BLOCKQUOTE"),
      run: (e) => (e.hasFormat("BLOCKQUOTE") ? e.decreaseQuoteLevel() : e.increaseQuoteLevel()),
    },
    {
      icon: "code",
      title: "Code (Ctrl+D)",
      active: (e) => e.hasFormat("CODE") || e.hasFormat("PRE"),
      run: (e) => e.toggleCode(),
    },
    { icon: "link", title: "Link", active: (e) => e.hasFormat("A"), run: link },
  ],
  [{ icon: "clearFormat", title: "Remove formatting", run: (e) => e.removeAllFormatting() }],
];

const ALIGN_ICONS: Record<string, IconName> = {
  left: "alignLeft",
  center: "alignCenter",
  right: "alignRight",
  justify: "alignJustify",
};

const IMPORTANCE = [
  { value: "high", label: "High importance" },
  { value: "normal", label: "Normal" },
  { value: "low", label: "Low importance" },
] as const;

/** The RFC 3676 signature separator, trailing space and all. */
const SEPARATOR = "-- ";

/** The configured signature: plain text, and the optional formatted version. */
type Signature = { plain: string; html: string | null };

/** The index of the last item matching `test`, or -1. */
function lastIndex<T>(items: T[], test: (item: T) => boolean): number {
  for (let i = items.length - 1; i >= 0; i--) if (test(items[i]!)) return i;
  return -1;
}

/** Whether a line or block is the signature separator, however the editor spaced it. */
function isSeparator(text: string): boolean {
  return text.replace(/\u00a0/g, " ").trim() === "--";
}

/**
 * Where a trailing quote begins, in a list of lines or blocks: the first of
 * the run at the end that is all quote or blank. The signature goes above it,
 * which is where a reply's author is signing.
 */
function quoteStart<T>(items: T[], isQuote: (item: T) => boolean, isBlank: (item: T) => boolean): number {
  let start = items.length;
  for (let i = items.length - 1; i >= 0; i--) {
    const item = items[i]!;
    if (isQuote(item)) start = i;
    else if (!isBlank(item)) break;
  }
  return start;
}

/** The plain body with the signature placed above any quote. */
function signPlain(text: string, signature: string): string {
  const lines = text.split("\n");
  const at = quoteStart(lines, (l) => l.startsWith(">"), (l) => l.trim() === "");
  const before = lines.slice(0, at);
  while (before.length > 1 && before[before.length - 1]!.trim() === "") before.pop();
  if (before.length === 0) before.push("");
  const after = lines.slice(at);
  return [...before, "", SEPARATOR, ...signature.split("\n"), ...(after.length ? ["", ...after] : [])].join(
    "\n",
  );
}

/** The plain body without its signature, or `null` if it has none. */
function unsignPlain(text: string): string | null {
  const lines = text.split("\n");
  const at = quoteStart(lines, (l) => l.startsWith(">"), (l) => l.trim() === "");
  const sep = lastIndex(lines.slice(0, at), isSeparator);
  if (sep < 0) return null;
  const before = lines.slice(0, sep);
  while (before.length > 1 && before[before.length - 1]!.trim() === "") before.pop();
  const after = lines.slice(at);
  return [...before, ...(after.length ? ["", ...after] : [])].join("\n");
}

/** The editor's top-level blocks, parsed inertly. */
function blocksOf(html: string): { root: DocumentFragment; blocks: Element[] } {
  const template = document.createElement("template");
  template.innerHTML = html;
  return { root: template.content, blocks: Array.from(template.content.children) };
}

const isQuoteBlock = (el: Element) =>
  el.tagName === "BLOCKQUOTE" || (el.textContent ?? "").trimStart().startsWith(">");
const isBlankBlock = (el: Element) => (el.textContent ?? "").replace(/\u00a0/g, " ").trim() === "";

/** Serialises a fragment back to markup for `setHTML`, which filters it again. */
function markupOf(root: DocumentFragment): string {
  const div = document.createElement("div");
  div.append(root);
  return div.innerHTML;
}

/** The formatted body with the signature placed above any quote. */
function signHtml(html: string, signatureHtml: string): string {
  const { root, blocks } = blocksOf(html);
  const at = quoteStart(blocks, isQuoteBlock, isBlankBlock);
  const insert = blocksOf(`<p><br></p><p>${SEPARATOR}</p>${signatureHtml}`).root;
  if (at < blocks.length) {
    insert.append(blocksOf("<p><br></p>").root);
    blocks[at]!.before(insert);
  } else {
    root.append(insert);
  }
  return markupOf(root);
}

/** The formatted body without its signature, or `null` if it has none. */
function unsignHtml(html: string): string | null {
  const { root, blocks } = blocksOf(html);
  const at = quoteStart(blocks, isQuoteBlock, isBlankBlock);
  const sep = lastIndex(blocks.slice(0, at), (b) => isSeparator(b.textContent ?? ""));
  if (sep < 0) return null;
  let first = sep;
  // The blank line the signature was set off by goes with it, but the line the
  // caret sits on at the top of an empty message does not.
  while (first > 1 && isBlankBlock(blocks[first - 1]!)) first--;
  for (const block of blocks.slice(first, at)) block.remove();
  return markupOf(root);
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/**
 * The editor of the composer on screen, if there is one.
 *
 * Kept so it can be destroyed: `paint()` replaces the whole screen on every
 * change, and Squire listens on the document as well as on its own root, so an
 * editor that is merely detached is still attached to something.
 */
let current: Squire | null = null;

function closeEditor(): void {
  closePopover();
  current?.destroy();
  current = null;
}

function toolButton(title: string, inner: string, attrs = ""): string {
  return `<button type="button" class="tool" title="${escapeHtml(title)}"
                  aria-label="${escapeHtml(title)}" ${attrs}>${inner}</button>`;
}

function renderToolbar(): string {
  let t = 0;
  return TOOLBAR.map(
    (group) =>
      `<div class="tool-group">${group
        .map((item) => {
          if ("menu" in item) {
            const inner =
              item.menu === "font"
                ? `<span class="font-label">Sans Serif</span>${icon("chevronDown", 14)}`
                : item.menu === "size"
                  ? `${icon("textSize")}${icon("chevronDown", 14)}`
                  : item.menu === "colour"
                    ? `${colourIcon()}${icon("chevronDown", 14)}`
                    : `<span class="align-icon">${icon("alignLeft")}</span>${icon("chevronDown", 14)}`;
            return toolButton(
              item.title,
              inner,
              `data-menu="${item.menu}" aria-haspopup="menu" aria-expanded="false"`,
            );
          }
          return toolButton(item.title, icon(item.icon), `data-tool="${t++}" aria-pressed="false"`);
        })
        .join("")}</div>`,
  ).join("");
}

export function renderComposer(el: HTMLElement): void {
  const draft: ComposerDraft = state.composerDraft ?? {
    to: "",
    cc: "",
    bcc: "",
    subject: "",
    body: "",
    html: null,
  };
  // Held in state from here on, so a repaint mid-sentence puts the composer
  // back as it was, including what the user decided about it.
  state.composerDraft = draft;
  let formatted = draft.html !== null;
  const lang = navigator.language || "en";

  el.innerHTML = `
    <form class="composer" id="composer" novalidate>
      <h1>New message</h1>
      <div class="fields">
        <div class="field">
          <label for="c-to">To</label>
          <input id="c-to" name="to" value="${escapeHtml(draft.to)}" autocomplete="off" spellcheck="false" />
          <span class="field-links">
            <button type="button" class="link" data-show="cc">Cc</button>
            <button type="button" class="link" data-show="bcc">Bcc</button>
          </span>
        </div>
        <div class="field" data-row="cc">
          <label for="c-cc">Cc</label>
          <input id="c-cc" name="cc" value="${escapeHtml(draft.cc)}" autocomplete="off" spellcheck="false" />
        </div>
        <div class="field" data-row="bcc">
          <label for="c-bcc">Bcc</label>
          <input id="c-bcc" name="bcc" value="${escapeHtml(draft.bcc)}" autocomplete="off" spellcheck="false" />
        </div>
        <div class="field">
          <label for="c-subject">Subject</label>
          <input id="c-subject" name="subject" value="${escapeHtml(draft.subject)}" autocomplete="off"
                 spellcheck="true" lang="${escapeHtml(lang)}" />
        </div>
      </div>
      <textarea name="body" rows="16" placeholder="Write your message" spellcheck="true"
                lang="${escapeHtml(lang)}" ${formatted ? "hidden" : ""}>${escapeHtml(draft.body)}</textarea>
      <div class="rich" id="rich" contenteditable="true" role="textbox" aria-multiline="true"
           aria-label="Message" spellcheck="true" lang="${escapeHtml(lang)}"
           ${formatted ? "" : "hidden"}></div>
      <div class="chips" id="attachment" hidden></div>
      <div class="crypto-note" id="crypto-note" role="status"></div>
      <div class="toolbar" id="toolbar" role="toolbar" aria-label="Formatting" ${formatted ? "" : "hidden"}>
        ${renderToolbar()}
      </div>
      <div class="send-bar">
        <button type="submit" class="send" title="Send (Ctrl+Enter)">Send</button>
        ${toolButton("Formatting options", icon("formatting"), `id="format-toggle" aria-pressed="${formatted}"`)}
        ${toolButton(
          "Attach a file. One per message: the engine carries one, and pretending otherwise here would move the surprise further from where you chose it.",
          icon("paperclip"),
          `id="attach"`,
        )}
        <input type="file" id="file" hidden />
        ${toolButton("Insert signature", icon("signature"), `id="signature" disabled`)}
        ${toolButton("Importance", icon("flag"), `id="importance" aria-haspopup="menu" aria-expanded="false"`)}
        ${toolButton("Encryption", icon("lock"), `id="lock" aria-pressed="true"`)}
        <span class="spacer"></span>
        ${toolButton("Discard", icon("trash"), `id="discard"`)}
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
  const input = (name: string) => form.elements.namedItem(name) as HTMLInputElement;
  const formatToggle = el.querySelector<HTMLButtonElement>("#format-toggle")!;
  const fileInput = el.querySelector<HTMLInputElement>("#file")!;
  const chips = el.querySelector<HTMLElement>("#attachment")!;
  const signatureButton = el.querySelector<HTMLButtonElement>("#signature")!;
  const importanceButton = el.querySelector<HTMLButtonElement>("#importance")!;
  const lockButton = el.querySelector<HTMLButtonElement>("#lock")!;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;

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

  /** Writes the fields, and the plain body, back into the draft. */
  const saveFields = () => {
    draft.to = input("to").value;
    draft.cc = input("cc").value;
    draft.bcc = input("bcc").value;
    draft.subject = input("subject").value;
    if (!formatted) draft.body = plain.value;
  };
  /** Writes the formatted body back into the draft. */
  const saveRich = () => {
    if (formatted && current === editor) draft.html = editor.getHTML();
  };
  // The editor's markup is serialised on a pause rather than per keystroke:
  // it is the whole message, every time.
  let richTimer: number | undefined;
  const onRichInput = () => {
    window.clearTimeout(richTimer);
    richTimer = window.setTimeout(() => {
      if (current !== editor) return;
      saveRich();
      paintSignatureButton();
    }, 300);
  };
  form.addEventListener("input", (event) => {
    if (rich.contains(event.target as Node)) onRichInput();
    else saveFields();
  });

  // --- Cc and Bcc: there when wanted, not before ---------------------------

  const showRows = () => {
    for (const row of ["cc", "bcc"] as const) {
      const shown = (row === "cc" ? draft.showCc : draft.showBcc) || input(row).value.trim() !== "";
      el.querySelector<HTMLElement>(`[data-row="${row}"]`)!.hidden = !shown;
      el.querySelector<HTMLElement>(`[data-show="${row}"]`)!.hidden = shown;
    }
  };
  showRows();
  for (const button of el.querySelectorAll<HTMLButtonElement>("[data-show]")) {
    button.addEventListener("click", () => {
      const row = button.dataset["show"] as "cc" | "bcc";
      if (row === "cc") draft.showCc = true;
      else draft.showBcc = true;
      showRows();
      input(row).focus();
    });
  }

  const readSet = (): RecipientSet => ({
    to: addresses(input("to").value),
    cc: addresses(input("cc").value),
    bcc: addresses(input("bcc").value),
  });

  // --- the padlock -------------------------------------------------------

  // Whether this will go out encrypted is the single most important thing about
  // a message, and the one thing the user cannot see once it is gone. So it is
  // asked of the engine while they type rather than reported afterwards, and
  // the answer is the engine's -- the same check `send_email` makes.
  let ready: SendReadiness | null = null;
  let hasRecipients = false;
  let asked = 0;
  let sending = false;

  const encryptOn = (): boolean =>
    ready?.locked ? true : (draft.encrypt ?? (ready ? ready.missing.length === 0 : true));
  /** Encryption is on and someone has no key: the engine would refuse it. */
  const blocked = (): boolean => encryptOn() && ready !== null && ready.missing.length > 0;

  const paintLock = () => {
    const on = encryptOn();
    lockButton.innerHTML = icon(on ? "lock" : "lockOpen");
    lockButton.setAttribute("aria-pressed", String(on));
    // Warning colours whenever the message cannot go out encrypted as it
    // stands: open, or closed with someone who has no key.
    lockButton.classList.toggle("warn", !on || blocked());
    lockButton.disabled = ready?.locked ?? false;
    const title = ready?.locked
      ? "Encrypted. Your settings require end-to-end encryption for these recipients."
      : on
        ? "Encrypted. Click to send this message unencrypted."
        : "Not encrypted. Click to encrypt this message.";
    lockButton.title = title;
    lockButton.setAttribute("aria-label", title);

    const missing = ready?.missing.join(", ") ?? "";
    if (!hasRecipients) {
      note.className = "crypto-note";
      note.textContent = "Add a recipient.";
    } else if (ready === null) {
      note.className = "crypto-note";
      note.textContent = "";
    } else if (on && missing === "") {
      note.className = "crypto-note good";
      note.innerHTML = `${icon("lock", 14)} Encrypted end to end.`;
    } else if (on) {
      note.className = "crypto-note warn";
      note.innerHTML =
        `${icon("lock", 14)} No key for ${escapeHtml(missing)}. ` +
        (ready.locked
          ? "These recipients are set to end-to-end only, so this cannot be sent until there is one."
          : "Turn encryption off to send it unencrypted, or remove them.");
    } else {
      note.className = "crypto-note warn";
      note.innerHTML =
        `${icon("lockOpen", 14)} Not encrypted: anyone who handles this message on its way can read it.` +
        (missing === "" ? " Everyone here has a key, so it could be." : "");
    }
    if (!sending) submit.disabled = blocked();
  };

  let readyTimer: number | undefined;
  const askReadiness = () => {
    window.clearTimeout(readyTimer);
    readyTimer = window.setTimeout(async () => {
      if (current !== editor) return;
      const set = readSet();
      hasRecipients = set.to.length + set.cc.length + set.bcc.length > 0;
      const mine = ++asked;
      if (!hasRecipients) {
        ready = null;
        paintLock();
        return;
      }
      try {
        const answer = (await rpc.call("get_send_readiness", [
          state.accountId,
          set,
        ])) as SendReadiness;
        // A slow answer about what the fields said a moment ago is not an
        // answer about what they say now.
        if (mine === asked) ready = answer;
      } catch {
        if (mine === asked) ready = null;
      }
      if (mine === asked) paintLock();
    }, 200);
  };
  for (const field of ["to", "cc", "bcc"]) {
    input(field).addEventListener("input", askReadiness);
  }
  paintLock();
  askReadiness();

  lockButton.addEventListener("click", () => {
    if (ready?.locked) return;
    draft.encrypt = !encryptOn();
    paintLock();
  });

  // --- formatting --------------------------------------------------------

  const setFormatted = (on: boolean) => {
    if (on === formatted) return;
    formatted = on;
    if (formatted) {
      // Carrying the text across rather than starting empty. Switching mode is
      // a decision about presentation, not about discarding what was written.
      editor.setHTML(fromText(plain.value));
      draft.html = editor.getHTML();
    } else {
      closePopover();
      plain.value = compose(editor.getHTML()).text;
      draft.body = plain.value;
      draft.html = null;
    }
    toolbar.hidden = !formatted;
    rich.hidden = !formatted;
    plain.hidden = formatted;
    formatToggle.setAttribute("aria-pressed", String(formatted));
    if (formatted) editor.focus();
    else plain.focus();
  };
  formatToggle.addEventListener("click", () => setFormatted(!formatted));

  const tools = TOOLBAR.flat().filter((item): item is Tool => !("menu" in item));
  const buttons = Array.from(toolbar.querySelectorAll<HTMLButtonElement>("button[data-tool]"));
  for (const button of buttons) {
    const tool = tools[Number(button.dataset["tool"])]!;
    onPress(button, () => {
      tool.run(editor);
      refreshToolbar();
    });
  }

  const fontLabel = toolbar.querySelector<HTMLElement>(".font-label")!;
  const alignIcon = toolbar.querySelector<HTMLElement>(".align-icon")!;
  const colourBar = toolbar.querySelector<SVGElement>(".colour-bar")!;

  const alignmentAt = (): string => {
    const node = editor.getSelection().startContainer;
    const element = node instanceof Element ? node : node.parentElement;
    const block = element?.closest("p, h1, h2, h3, li, blockquote, pre");
    return ALIGNMENTS.find((a) => block?.classList.contains(`align-${a}`)) ?? "left";
  };

  // Pressed state, font name, colour bar and alignment follow the caret, so
  // the toolbar says what a click will do.
  const refreshToolbar = () => {
    buttons.forEach((button) => {
      const tool = tools[Number(button.dataset["tool"])]!;
      if (tool.active) button.setAttribute("aria-pressed", String(tool.active(editor)));
    });
    const info = editor.getFontInfo();
    const family = (info["fontFamily"] ?? "").split(",")[0]!.trim().replace(/^["']|["']$/g, "");
    fontLabel.textContent = FONTS.find((f) => f.value === family)?.label ?? "Sans Serif";
    colourBar.style.fill = info["color"] ?? "currentColor";
    alignIcon.innerHTML = icon(ALIGN_ICONS[alignmentAt()] ?? "alignLeft");
  };
  editor.addEventListener("pathChange", refreshToolbar);

  /** A menu item that runs `run` on the editor, then closes its menu. */
  const menuItem = (label: string, run: () => void): HTMLButtonElement => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "menu-item";
    item.setAttribute("role", "menuitem");
    item.innerHTML = label;
    onPress(item, () => {
      run();
      closePopover();
      editor.focus();
      refreshToolbar();
    });
    return item;
  };

  const panel = (className: string): HTMLElement => {
    const div = document.createElement("div");
    div.className = className;
    return div;
  };

  const fontMenu = (): HTMLElement => {
    const menu = panel("menu");
    menu.append(menuItem("Default", () => editor.setFontFace(null)));
    for (const font of FONTS) {
      const item = menuItem(escapeHtml(font.label), () => editor.setFontFace(font.value));
      item.style.fontFamily = font.value;
      menu.append(item);
    }
    return menu;
  };

  const sizeMenu = (): HTMLElement => {
    const menu = panel("menu");
    const sizes = [SIZES[0]!, { label: "Normal", value: "" }, ...SIZES.slice(1)];
    for (const size of sizes) {
      const item = menuItem(escapeHtml(size.label), () => editor.setFontSize(size.value || null));
      if (size.value) item.style.fontSize = size.value;
      menu.append(item);
    }
    return menu;
  };

  const colourColumn = (
    title: string,
    reset: string,
    apply: (colour: string | null) => void,
  ): HTMLElement => {
    const column = panel("colour-column");
    const heading = document.createElement("div");
    heading.className = "colour-title";
    heading.textContent = title;
    column.append(heading);
    column.append(menuItem(escapeHtml(reset), () => apply(null)));
    const grid = panel("swatches");
    for (const row of PALETTE) {
      for (const colour of row) {
        const swatch = menuItem("", () => apply(colour.value));
        swatch.className = "swatch";
        swatch.title = colour.label;
        swatch.setAttribute("aria-label", colour.label);
        swatch.style.background = colour.value;
        grid.append(swatch);
      }
    }
    column.append(grid);
    // Any colour survives the filter, so a custom one is only a matter of
    // asking. The native picker takes focus; Squire keeps the selection it
    // had while blurred, so the colour still lands on it.
    const custom = document.createElement("label");
    custom.className = "custom-colour";
    custom.innerHTML = `Custom… <input type="color" value="#1f5fbf" />`;
    custom.querySelector("input")!.addEventListener("change", (event) => {
      apply((event.target as HTMLInputElement).value);
      closePopover();
      editor.focus();
      refreshToolbar();
    });
    column.append(custom);
    return column;
  };

  const colourMenu = (): HTMLElement => {
    const menu = panel("colour-menu");
    menu.append(
      colourColumn("Text colour", "Default", (c) => editor.setTextColor(c)),
      colourColumn("Highlight", "None", (c) => editor.setHighlightColor(c)),
    );
    return menu;
  };

  const alignMenu = (): HTMLElement => {
    const menu = panel("menu row");
    for (const align of ALIGNMENTS) {
      const item = menuItem(icon(ALIGN_ICONS[align]!), () => editor.setTextAlignment(align));
      item.title = `Align ${align}`;
      item.setAttribute("aria-label", item.title);
      item.setAttribute("aria-checked", String(alignmentAt() === align));
      menu.append(item);
    }
    return menu;
  };

  const MENUS = { font: fontMenu, size: sizeMenu, colour: colourMenu, align: alignMenu };
  for (const button of toolbar.querySelectorAll<HTMLButtonElement>("button[data-menu]")) {
    const build = MENUS[button.dataset["menu"] as keyof typeof MENUS];
    onPress(button, () => togglePopover(button, build));
  }

  // --- the signature -----------------------------------------------------

  let signature: Signature | null = null;
  // Whether the composer is in charge of the signature for this message. Only
  // once it has been read: if it could not be, the engine appends it as it
  // always has rather than the message going out unsigned.
  let signatureKnown = false;

  const bodyHasSignature = (): boolean =>
    formatted ? unsignHtml(editor.getHTML()) !== null : unsignPlain(plain.value) !== null;

  const paintSignatureButton = () => {
    signatureButton.disabled = signature === null;
    const title =
      signature === null
        ? "No signature set. Add one in Settings."
        : bodyHasSignature()
          ? "Remove signature"
          : "Insert signature";
    signatureButton.title = title;
    signatureButton.setAttribute("aria-label", title);
  };

  const toggleSignature = () => {
    if (signature === null) return;
    if (formatted) {
      const html = editor.getHTML();
      editor.setHTML(unsignHtml(html) ?? signHtml(html, signature.html ?? fromText(signature.plain)));
      draft.html = editor.getHTML();
    } else {
      plain.value = unsignPlain(plain.value) ?? signPlain(plain.value, signature.plain);
      draft.body = plain.value;
    }
    paintSignatureButton();
  };
  signatureButton.addEventListener("click", toggleSignature);

  void (async () => {
    try {
      const [text, html] = (await Promise.all([
        rpc.call("get_config", [state.accountId, "email_signature"]),
        rpc.call("get_config", [state.accountId, "email_signature_html"]),
      ])) as [string | null, string | null];
      signature = text?.trim() ? { plain: text.trimEnd(), html: html?.trim() ? html : null } : null;
      signatureKnown = true;
    } catch {
      return;
    }
    if (current !== editor) return;
    // Placed once per message, the way other clients do: a new message opens
    // signed, and a signature the user then deleted stays deleted.
    if (!draft.signed) {
      draft.signed = true;
      if (signature !== null && !bodyHasSignature()) {
        toggleSignature();
        // The caret at the top, above the signature, where the message goes.
        if (formatted) editor.moveCursorToStart();
        else plain.setSelectionRange(0, 0);
      }
    }
    paintSignatureButton();
  })();
  plain.addEventListener("input", paintSignatureButton);

  // --- the attachment ----------------------------------------------------

  const paintAttachment = () => {
    const file = draft.attachment ?? null;
    chips.hidden = file === null;
    if (file === null) {
      chips.innerHTML = "";
      return;
    }
    chips.innerHTML = `
      <span class="chip">${icon("file", 16)}
        <span class="chip-name">${escapeHtml(file.name)}</span>
        <span class="chip-size">${formatBytes(file.size)}</span>
        <button type="button" class="chip-remove" title="Remove attachment"
                aria-label="Remove attachment">${icon("x", 14)}</button>
      </span>`;
    chips.querySelector<HTMLButtonElement>(".chip-remove")!.addEventListener("click", () => {
      draft.attachment = null;
      paintAttachment();
    });
  };
  paintAttachment();
  el.querySelector<HTMLButtonElement>("#attach")!.addEventListener("click", () => fileInput.click());
  fileInput.addEventListener("change", () => {
    // Picking again replaces: one file per message, and the chip says which.
    draft.attachment = fileInput.files?.[0] ?? draft.attachment ?? null;
    fileInput.value = "";
    paintAttachment();
  });

  // --- importance --------------------------------------------------------

  const paintImportance = () => {
    const level = draft.importance ?? "normal";
    importanceButton.innerHTML = icon(level === "low" ? "arrowDown" : "flag");
    importanceButton.dataset["level"] = level;
    importanceButton.setAttribute("aria-pressed", String(level !== "normal"));
    const title = `Importance: ${level}`;
    importanceButton.title = title;
    importanceButton.setAttribute("aria-label", title);
  };
  paintImportance();
  onPress(importanceButton, () =>
    togglePopover(importanceButton, () => {
      const menu = panel("menu");
      for (const option of IMPORTANCE) {
        const checked = (draft.importance ?? "normal") === option.value;
        const item = menuItem(
          `<span class="menu-check">${checked ? icon("check", 14) : ""}</span>${escapeHtml(option.label)}`,
          () => {
            // Normal puts no header on the message at all, which is what keeps
            // an ordinary message identical to one sent before this control
            // existed.
            draft.importance = option.value;
            paintImportance();
          },
        );
        item.setAttribute("role", "menuitemradio");
        item.setAttribute("aria-checked", String(checked));
        menu.append(item);
      }
      return menu;
    }),
  );

  // --- sending -----------------------------------------------------------

  form.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      if (!submit.disabled) form.requestSubmit();
    }
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
    if (blocked()) return;

    saveFields();
    saveRich();
    const body = formatted ? compose(editor.getHTML()) : { text: plain.value, html: null };
    const options: SendOptions = {
      // Never "auto" from here: the padlock always says something, and the
      // engine refuses what the policy does not allow either way.
      encryption: encryptOn() ? "required" : "plaintext",
      signatureInBody: signatureKnown,
    };
    const file = draft.attachment ?? null;
    sending = true;
    submit.disabled = true;
    submit.textContent = "Sending…";

    try {
      await rpc.call("send_email", [
        state.accountId,
        set,
        input("subject").value,
        // The plain-text part, always. `html` is an alternative beside it, so a
        // correspondent whose client shows plain text reads the message.
        body.text,
        // A File in the renderer has no filesystem path, so the shell stages
        // the bytes and hands back one.
        file ? await stageAttachment(file) : null,
        body.html,
        draft.importance ?? "normal",
        options,
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
      sending = false;
      submit.textContent = "Send";
      submit.disabled = blocked();
    }
  });

  el.querySelector<HTMLButtonElement>("#discard")!.addEventListener("click", () => {
    closeEditor();
    state.composerDraft = null;
    state.screen = null;
    changed();
  });
}
