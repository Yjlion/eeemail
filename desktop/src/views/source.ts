/**
 * The original message source, as it arrived.
 *
 * eeemail keeps the raw bytes of every message it sends and receives
 * (`core/src/email/rawmime.rs`), because the local store is the only durable
 * copy of the mailbox. Until now the reading pane said so in a footnote and
 * gave no way to look, which is a strange thing for a client whose case for
 * storing them is that they are the evidence.
 *
 * Rendered as text into a `<pre>`, never parsed and never framed: this is the
 * app document, and the whole point of the view is that nothing interprets the
 * bytes.
 */

import { rpc } from "../client";
import { state } from "../state";
import { escapeHtml } from "../html";

/** Shows the raw source of a message, or says why it cannot. */
export async function showSource(msgId: number): Promise<void> {
  const source = (await rpc.call("get_message_raw_mime", [
    state.accountId,
    msgId,
  ])) as string | null;

  const overlay = document.createElement("div");
  overlay.className = "overlay";
  overlay.innerHTML = `
    <div class="overlay-panel" role="dialog" aria-modal="true" aria-label="Message source">
      <div class="overlay-head">
        <h2>Message source</h2>
        <div class="overlay-actions">
          ${source === null ? "" : `<button data-act="copy">Copy</button>`}
          <button data-act="close">Close</button>
        </div>
      </div>
      ${
        source === null
          ? `<div class="notice warn">The original source of this message has
               expired and is no longer available. What is left is the decoded
               message, not the bytes that arrived.</div>`
          : `<pre class="source">${escapeHtml(source)}</pre>
             <div class="footnote">Invalid byte sequences are shown replaced,
               because this has to be text to reach the screen. Export the
               message to get exactly what arrived.</div>`
      }
    </div>`;

  const close = () => {
    overlay.remove();
    document.removeEventListener("keydown", onKey, true);
  };
  function onKey(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  }

  overlay.addEventListener("mousedown", (event) => {
    // Only a press on the backdrop itself. A drag that starts inside the panel
    // and ends outside it must not count as dismissing.
    if (event.target === overlay) close();
  });
  overlay.querySelector<HTMLButtonElement>("button[data-act='close']")!.addEventListener(
    "click",
    close,
  );
  overlay
    .querySelector<HTMLButtonElement>("button[data-act='copy']")
    ?.addEventListener("click", () => {
      void navigator.clipboard.writeText(source ?? "");
    });
  document.addEventListener("keydown", onKey, true);

  document.body.append(overlay);
  overlay.querySelector<HTMLButtonElement>("button[data-act='close']")!.focus();
}
