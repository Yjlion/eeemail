/**
 * Turning an edited body into mail.
 *
 * The composer edits a `contenteditable`, which means the browser decides what
 * markup gets produced and what a paste drops into it. Neither is something to
 * put on the wire. So nothing the editor produced is sent: this module walks
 * the DOM itself and re-emits it as a small, fixed set of tags, and derives the
 * plain-text alternative from the same walk so the two cannot disagree.
 *
 * The tag set is Delta Chat's, which their clients converged on and which
 * upstream's reading path already renders. Deliberately small: every tag here
 * is one a recipient's client has to handle, and eeemail's own reading pane
 * shows message HTML in a sandboxed frame precisely because HTML in mail is not
 * a safe format. Sending less of it than we are willing to receive is the
 * consistent position.
 *
 * See `docs/adr/0025-composed-html.md`.
 */

/** What may reach the wire. Anything else is unwrapped, keeping its text. */
const ALLOWED = new Set([
  "B",
  "I",
  "U",
  "S",
  "H1",
  "H2",
  "H3",
  "UL",
  "OL",
  "LI",
  "BLOCKQUOTE",
  "CODE",
  "PRE",
  "P",
  "BR",
  "A",
]);

/** Tags the browser's editor emits for things in [`ALLOWED`] under another name. */
const RENAMED: Record<string, string> = {
  STRONG: "B",
  EM: "I",
  STRIKE: "S",
  DEL: "S",
  INS: "U",
  DIV: "P",
};

/** Elements that end a line when the message is read as plain text. */
const BLOCKS = new Set(["P", "H1", "H2", "H3", "LI", "BLOCKQUOTE", "PRE", "UL", "OL"]);

/** Escapes text for an HTML body. */
function escape(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/**
 * Whether a link target may be written into a message.
 *
 * `http`, `https` and `mailto` only. A `javascript:` or `data:` href in an
 * outgoing message is an attack on whoever opens it, and this is the last point
 * at which we are the ones deciding.
 *
 * **Parsed with no base URL, deliberately.** Resolving against a placeholder
 * would turn a scheme-less href into an absolute link to whatever that
 * placeholder was -- a dead address we invented, sent to the recipient looking
 * like one the user chose. A relative href reaches the wire in no meaningful
 * sense anyway: mail has no document to be relative to. So anything without a
 * scheme we recognise becomes its own text, and [`normalizeHref`] is what gives
 * the user's typing a scheme in the first place.
 */
function safeHref(href: string): string | null {
  let url: URL;
  try {
    url = new URL(href);
  } catch {
    return null;
  }
  if (url.protocol === "http:" || url.protocol === "https:" || url.protocol === "mailto:") {
    return url.href;
  }
  return null;
}

/**
 * Gives what someone typed at a link prompt the scheme they meant.
 *
 * People type `example.com`, not `https://example.com`, and they type a bare
 * address when they mean to write to someone. Guessing here -- once, at the
 * point the user expressed the intent -- is what keeps [`safeHref`] a plain
 * whitelist with no guessing in it.
 *
 * Anything that already carries a scheme is returned untouched, including a
 * `javascript:` one: this function decides what the user meant, not what is
 * allowed, and `safeHref` still has the last word on send.
 */
export function normalizeHref(input: string): string {
  const href = input.trim();
  if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return href;
  // An address, not a host: `https://` on this would read the local part as a
  // username and quietly link somewhere else entirely.
  if (/^[^\s/@]+@[^\s/@]+\.[^\s/@]+$/.test(href)) return `mailto:${href}`;
  return href ? `https://${href}` : href;
}

/** Re-emits `node` and its children as allowed tags only. */
function toHtml(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) {
    return escape(node.nodeValue ?? "");
  }
  if (node.nodeType !== Node.ELEMENT_NODE) return "";

  const el = node as Element;
  const tag = RENAMED[el.tagName] ?? el.tagName;
  const inner = Array.from(el.childNodes).map(toHtml).join("");

  if (tag === "BR") return "<br>";
  if (!ALLOWED.has(tag)) {
    // Unwrapped, not dropped. A `<span style=…>` the editor added around a word
    // carries the word, and losing it would lose what the user typed.
    return inner;
  }
  if (tag === "A") {
    const href = safeHref(el.getAttribute("href") ?? "");
    // A link we will not carry becomes its own text, so the address is still
    // readable rather than silently gone.
    if (href === null) return inner;
    return `<a href="${escape(href)}">${inner}</a>`;
  }
  const name = tag.toLowerCase();
  return `<${name}>${inner}</${name}>`;
}

/** Renders `node` and its children as the plain-text alternative. */
function toText(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return node.nodeValue ?? "";
  if (node.nodeType !== Node.ELEMENT_NODE) return "";

  const el = node as Element;
  const tag = RENAMED[el.tagName] ?? el.tagName;
  if (tag === "BR") return "\n";

  const inner = Array.from(el.childNodes).map(toText).join("");
  if (tag === "LI") return `- ${inner}\n`;
  if (tag === "BLOCKQUOTE") {
    return `${inner
      .split("\n")
      .map((line) => (line ? `> ${line}` : ">"))
      .join("\n")}\n`;
  }
  if (BLOCKS.has(tag)) return `${inner}\n`;
  return inner;
}

/** An edited body, as the two parts a message carries. */
export type Composed = {
  /** The `text/plain` part. Always sent, and never optional. */
  text: string;
  /** The `text/html` alternative, or `null` when the body is unformatted. */
  html: string | null;
};

/**
 * Reads a `contenteditable` as the two parts of a message.
 *
 * `html` is `null` when the walk produced nothing a plain-text reader would
 * miss, so an unformatted message composed in formatted mode still goes out as
 * an ordinary plain-text mail rather than as HTML that says the same thing.
 */
export function compose(root: HTMLElement): Composed {
  const html = Array.from(root.childNodes).map(toHtml).join("").trim();
  const text = Array.from(root.childNodes)
    .map(toText)
    .join("")
    .replace(/\n{3,}/g, "\n\n")
    .trim();

  // `<p>` and `<br>` alone carry no formatting a plain-text reader loses.
  const formatted = /<(?!\/?(?:p|br)\b)[a-z]/i.test(html);
  return { text, html: formatted ? html : null };
}

/** Renders plain text back into the editor, for the plain-to-formatted switch. */
export function fromText(text: string): string {
  return text
    .split("\n")
    .map((line) => `<p>${escape(line) || "<br>"}</p>`)
    .join("");
}
