/**
 * Turning an edited body into mail, and anything else into an edited body.
 *
 * The composer edits with Squire, which means a library decides what markup
 * gets produced and a paste can drop in whatever the source page had. Neither
 * is something to put on the wire. So nothing the editor produced is sent: this
 * module walks the DOM itself and re-emits it as a small, fixed set of tags and
 * styles, and derives the plain-text alternative from the same walk so the two
 * cannot disagree.
 *
 * The same walk is also the editor's way in. Squire hands every paste and every
 * `setHTML` to [`sanitizeToFragment`], so what the user sees while writing has
 * already been through the filter that decides what is sent -- there is one
 * whitelist, not a paste sanitiser and a send sanitiser that can drift apart.
 *
 * The tag set is Delta Chat's, which their clients converged on and which
 * upstream's reading path already renders, plus a `style` attribute that is
 * *rebuilt* from a handful of checked properties and never copied. Every tag
 * here is one a recipient's client has to handle, and eeemail's own reading pane
 * shows message HTML in a sandboxed frame precisely because HTML in mail is not
 * a safe format.
 *
 * See `docs/adr/0025-composed-html.md` and `docs/adr/0030-the-composer-edits-with-squire.md`.
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
  "SPAN",
]);

/**
 * Elements removed together with their content.
 *
 * Unwrapping keeps text, which is right for a `<span>` and wrong for these: the
 * text of a `<style>` or `<script>` is not something the user wrote, and a paste
 * of a whole page would otherwise put its stylesheet into the message.
 */
const DROPPED = new Set([
  "SCRIPT",
  "STYLE",
  "TEMPLATE",
  "TITLE",
  "META",
  "LINK",
  "NOSCRIPT",
  "IFRAME",
  "OBJECT",
  "EMBED",
  "SVG",
  "MATH",
  "SELECT",
  "TEXTAREA",
]);

/** Tags the editor or a paste emits for things in [`ALLOWED`] under another name. */
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

/** Blocks that may carry an alignment. */
const ALIGNABLE = new Set(["P", "H1", "H2", "H3", "LI", "BLOCKQUOTE", "PRE"]);

/**
 * The font families the toolbar offers, and the only ones that are sent.
 *
 * Generic families only. A named font is a guess about what the recipient has
 * installed, and a free-text family is the one style value that could carry
 * anything at all.
 */
export const FONTS = [
  { label: "Sans-serif", value: "sans-serif" },
  { label: "Serif", value: "serif" },
  { label: "Monospace", value: "monospace" },
];

/** The font sizes the toolbar offers, and the only ones that are sent. Keywords, so they scale with the reader's default. */
export const SIZES = [
  { label: "Small", value: "small" },
  { label: "Large", value: "large" },
  { label: "Huge", value: "x-large" },
];

/** The text colours the toolbar offers. Any colour survives the filter; these are what is offered. */
export const COLOURS = [
  { label: "Red", value: "#c0392b" },
  { label: "Orange", value: "#d35400" },
  { label: "Green", value: "#1e8449" },
  { label: "Blue", value: "#1f5fbf" },
  { label: "Purple", value: "#7d3c98" },
  { label: "Grey", value: "#6b7280" },
];

/** The highlight colours the toolbar offers. */
export const HIGHLIGHTS = [
  { label: "Yellow", value: "#fff3a3" },
  { label: "Green", value: "#c8f0c8" },
  { label: "Blue", value: "#cde3ff" },
  { label: "Pink", value: "#fbd3e9" },
];

export const ALIGNMENTS = ["left", "center", "right", "justify"];

/** Who the walk is producing markup for. */
type Mode = "wire" | "editor";

/**
 * A colour, if it is only a colour.
 *
 * The value has already been through the browser's CSS parser, which
 * serialises a hex colour as `rgb()` and leaves a keyword as it is. Accepting
 * nothing inside the parentheses but digits and separators, and nothing but
 * letters outside them, is what keeps a `url(` or an `expression(` from riding
 * along.
 */
function colour(value: string): string | null {
  const v = value.trim().toLowerCase();
  // The CSS-wide keywords are letters too, and are what a `background:`
  // shorthand leaves in `background-color`. They are not a colour anyone chose.
  if (/^(?:initial|inherit|unset|revert|revert-layer)$/.test(v)) return null;
  return /^rgba?\([\d\s.,%/]+\)$/.test(v) || /^#[0-9a-f]{3,8}$/.test(v) || /^[a-z]+$/.test(v)
    ? v
    : null;
}

/** The first family of a `font-family` list, if it is one the toolbar offers. */
function fontFamily(value: string): string | null {
  const first = (value.split(",")[0] ?? "").trim().replace(/^["']|["']$/g, "").toLowerCase();
  return FONTS.some((font) => font.value === first) ? first : null;
}

/** A `font-size`, if it is one the toolbar offers. */
function fontSize(value: string): string | null {
  const v = value.trim().toLowerCase();
  return SIZES.some((size) => size.value === v) ? v : null;
}

/**
 * The inline styles a `<span>` may carry.
 *
 * `className` is the class Squire marks each one with and finds it by when
 * replacing or removing it. It is written back only for the editor, and derived
 * from which property survived rather than copied from the input.
 */
const SPAN_STYLES: { property: string; className: string; check: (v: string) => string | null }[] = [
  { property: "color", className: "color", check: colour },
  { property: "background-color", className: "highlight", check: colour },
  { property: "font-family", className: "font", check: fontFamily },
  { property: "font-size", className: "size", check: fontSize },
];

/** Escapes text for an HTML body or a quoted attribute. */
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

/** The canonical name of an element, whatever namespace or spelling it arrived in. */
function tagOf(el: Element): string {
  const tag = el.tagName.toUpperCase();
  return RENAMED[tag] ?? tag;
}

/** An element's inline style, or `null` for one with no style to read. */
function styleOf(el: Element): CSSStyleDeclaration | null {
  return el instanceof HTMLElement ? el.style : null;
}

/** Re-emits `node` and its children as allowed tags and styles only. */
function toHtml(node: Node, mode: Mode): string {
  if (node.nodeType === Node.TEXT_NODE) {
    return escape(node.nodeValue ?? "");
  }
  if (node.nodeType !== Node.ELEMENT_NODE) return "";

  const el = node as Element;
  const tag = tagOf(el);
  if (DROPPED.has(tag)) return "";
  const inner = Array.from(el.childNodes)
    .map((child) => toHtml(child, mode))
    .join("");

  if (tag === "BR") return "<br>";
  if (!ALLOWED.has(tag)) {
    // Unwrapped, not dropped. A `<font>` or `<section>` from a paste carries the
    // words, and losing it would lose what the user wrote.
    return inner;
  }
  if (tag === "A") {
    const href = safeHref(el.getAttribute("href") ?? "");
    // A link we will not carry becomes its own text, so the address is still
    // readable rather than silently gone.
    if (href === null) return inner;
    return `<a href="${escape(href)}">${inner}</a>`;
  }

  const style = styleOf(el);
  if (tag === "SPAN") {
    const kept = SPAN_STYLES.flatMap(({ property, className, check }) => {
      const value = check(style?.getPropertyValue(property) ?? "");
      return value === null ? [] : [{ css: `${property}:${value}`, className }];
    });
    // A span is only ever its style. With none left it is just its text.
    if (kept.length === 0) return inner;
    if (mode === "wire") {
      return `<span style="${escape(kept.map((k) => k.css).join(";"))}">${inner}</span>`;
    }
    // One span per property for the editor, which is the shape Squire makes and
    // the only one it can find again to change or remove.
    return kept.reduceRight(
      (html, k) => `<span class="${k.className}" style="${escape(k.css)}">${html}</span>`,
      inner,
    );
  }

  const name = tag.toLowerCase();
  const align = ALIGNABLE.has(tag) ? (style?.getPropertyValue("text-align") ?? "").trim() : "";
  if (ALIGNMENTS.includes(align)) {
    const cls = mode === "editor" ? ` class="align-${align}"` : "";
    return `<${name}${cls} style="text-align:${align}">${inner}</${name}>`;
  }
  return `<${name}>${inner}</${name}>`;
}

/** Renders `node` and its children as the plain-text alternative. */
function toText(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return node.nodeValue ?? "";
  if (node.nodeType !== Node.ELEMENT_NODE) return "";

  const el = node as Element;
  const tag = tagOf(el);
  if (DROPPED.has(tag)) return "";
  if (tag === "BR") {
    // The `<br>` an editor keeps at the end of a block so the caret has a line
    // to sit on is not a line of its own; the block already ends one.
    const parent = el.parentElement;
    const trailing = el.nextSibling === null && parent !== null && BLOCKS.has(tagOf(parent));
    return trailing ? "" : "\n";
  }

  const inner = Array.from(el.childNodes).map(toText).join("");
  if (tag === "LI") return `- ${inner.replace(/\n+$/, "")}\n`;
  if (tag === "BLOCKQUOTE") {
    return `${inner
      .replace(/\n+$/, "")
      .split("\n")
      .map((line) => (line ? `> ${line}` : ">"))
      .join("\n")}\n`;
  }
  if (BLOCKS.has(tag)) return `${inner}\n`;
  return inner;
}

/** Parses markup into an inert fragment: no script runs and nothing loads. */
function parse(html: string): DocumentFragment {
  const template = document.createElement("template");
  template.innerHTML = html;
  return template.content;
}

/**
 * Squire's `sanitizeToDOMFragment`: every paste and every `setHTML` comes
 * through here.
 *
 * Parsed twice on purpose. The first parse is inert and only ever walked; it is
 * never put into the document, because adopting an `<img onerror>` into the
 * live document is enough to fire it. What is imported is a second parse of the
 * walk's output, which by construction has no attribute but a checked `href`,
 * `style` and a class we wrote.
 */
export function sanitizeToFragment(html: string): DocumentFragment {
  const clean = Array.from(parse(html).childNodes)
    .map((node) => toHtml(node, "editor"))
    .join("");
  return document.importNode(parse(clean), true);
}

/** An edited body, as the two parts a message carries. */
export type Composed = {
  /** The `text/plain` part. Always sent, and never optional. */
  text: string;
  /** The `text/html` alternative, or `null` when the body is unformatted. */
  html: string | null;
};

/**
 * Reads the editor's markup as the two parts of a message.
 *
 * Takes Squire's `getHTML()` rather than its live root, which is the same
 * content without the zero-width spaces and selection markers Squire keeps in
 * the document while editing.
 *
 * `html` is `null` when the walk produced nothing a plain-text reader would
 * miss, so an unformatted message composed in formatted mode still goes out as
 * an ordinary plain-text mail rather than as HTML that says the same thing.
 */
export function compose(editorHtml: string): Composed {
  const nodes = Array.from(parse(editorHtml).childNodes);
  const html = nodes
    .map((node) => toHtml(node, "wire"))
    .join("")
    .trim();
  const text = nodes
    .map(toText)
    .join("")
    .replace(/\n{3,}/g, "\n\n")
    .trim();

  // A bare `<p>` or `<br>` carries no formatting a plain-text reader loses. An
  // aligned `<p style=…>` does, which is why the test is on the closing `>`.
  const formatted = /<(?!\/?(?:p|br)>)/i.test(html);
  return { text, html: formatted ? html : null };
}

/** Renders plain text back into the editor, for the plain-to-formatted switch. */
export function fromText(text: string): string {
  return text
    .split("\n")
    .map((line) => `<p>${escape(line) || "<br>"}</p>`)
    .join("");
}
