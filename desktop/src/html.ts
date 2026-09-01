/**
 * Rendering untrusted message content.
 *
 * Every message body in the reading pane came from a stranger. Two rules follow
 * and neither is negotiable:
 *
 * 1. **Nothing in a message may reach the network.** A single remote image is a
 *    read receipt the sender gets whether the user consented or not, plus the
 *    user's IP address. The window CSP already forbids remote origins; this
 *    module strips remote references anyway, so a CSP mistake is not a privacy
 *    breach on its own.
 * 2. **Message HTML never runs in the app document.** It is rendered inside a
 *    sandboxed iframe with a `null` origin, so even a successful injection has
 *    no access to the RPC pipe, the account, or the rest of the DOM.
 *
 * See `docs/adr/0013-desktop-ui.md`.
 */

/** Escapes text for safe insertion into HTML. */
export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/**
 * Renders plain text as HTML, keeping line breaks and linkifying URLs.
 *
 * Links are rendered as text, not anchors: an anchor in the reading pane is a
 * click away from leaving the sandbox, and opening external links is a
 * deliberate action the shell should mediate rather than something the message
 * decides.
 */
export function renderPlainText(text: string): string {
  return escapeHtml(text).replace(/\n/g, "<br>");
}

/**
 * Wraps message HTML in a sandboxed document.
 *
 * `sandbox` with no `allow-*` tokens means: no scripts, no forms, no top-level
 * navigation, and a `null` origin. `srcdoc` rather than a `blob:` URL so the
 * frame cannot be reached by anything else.
 */
export function sandboxedDocument(bodyHtml: string): string {
  const doc = `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src 'unsafe-inline'">
<style>
  body { font: 14px/1.5 system-ui, sans-serif; color: #1a1a1a; margin: 0; padding: 12px; }
  img { max-width: 100%; }
  blockquote { border-left: 3px solid #ccc; margin: 0 0 0 4px; padding-left: 12px; color: #555; }
  pre { white-space: pre-wrap; }
</style></head><body>${bodyHtml}</body></html>`;
  return doc;
}

/** True if a message body references anything that would be fetched remotely. */
export function hasRemoteContent(html: string): boolean {
  return /\s(?:src|href|srcset)\s*=\s*["']?\s*(?:https?:|\/\/)/i.test(html);
}
