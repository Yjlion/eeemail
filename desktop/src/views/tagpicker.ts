/**
 * Putting tags on one message.
 *
 * A separate overlay rather than a submenu: `menu.ts` renders a flat list and
 * resolves with one `act`, and teaching it to nest so that a tag list could
 * hang off one row would be more machinery than the feature. This also lets a
 * message be tagged and untagged several times without reopening anything,
 * which a menu that closes on the first click cannot do.
 *
 * Writes go through as they are clicked, not on a Done button. A checkbox that
 * has visibly changed and not been saved is the state this has to avoid.
 */

import { rpc } from "../client";
import { state } from "../state";
import { escapeHtml } from "../html";
import type { Label, MessageTags } from "../types";

/**
 * Shows the tag picker for a message. Resolves once it closes, with whether
 * anything was applied or removed -- the caller reloads the list on `true`,
 * because a tag change can move a message out of the view showing it.
 */
export async function showTagPicker(msgId: number): Promise<boolean> {
  const account = state.accountId;
  const [labels, tags] = (await Promise.all([
    rpc.call("get_labels", [account]),
    rpc.call("get_message_tags", [account, msgId]),
  ])) as [Label[], MessageTags];

  const userLabels = labels.filter((l) => !l.isSystem);
  const applied = new Set(tags.user.map((l) => l.id));
  let changedAnything = false;

  const overlay = document.createElement("div");
  overlay.className = "overlay";
  overlay.innerHTML = `
    <div class="overlay-panel" role="dialog" aria-modal="true" aria-label="Tags">
      <div class="overlay-head">
        <h2>Tags</h2>
        <div class="overlay-actions">
          <button data-act="close">Close</button>
        </div>
      </div>
      ${
        userLabels.length
          ? `<div class="tag-picker">
              ${userLabels
                .map(
                  (l) => `
                <label class="check">
                  <input type="checkbox" data-label-id="${l.id}"${applied.has(l.id) ? " checked" : ""} />
                  <span class="dot" style="background:${escapeHtml(l.color ?? "transparent")}"></span>
                  ${escapeHtml(l.name)}
                </label>`,
                )
                .join("")}
             </div>`
          : `<div class="empty small">
               No tags yet. Make one in <strong>Manage tags…</strong> in the sidebar.
             </div>`
      }
      <div class="error" id="tag-picker-error" hidden></div>
    </div>`;

  const error = overlay.querySelector<HTMLElement>("#tag-picker-error")!;

  return new Promise((resolve) => {
    const close = () => {
      overlay.remove();
      document.removeEventListener("keydown", onKey, true);
      resolve(changedAnything);
    };
    function onKey(event: KeyboardEvent): void {
      if (event.key === "Escape") {
        event.preventDefault();
        close();
      }
    }

    for (const box of overlay.querySelectorAll<HTMLInputElement>("input[data-label-id]")) {
      box.addEventListener("change", async () => {
        const id = Number(box.dataset["labelId"]);
        const method = box.checked ? "apply_label" : "unapply_label";
        try {
          await rpc.call(method, [account, [msgId], id]);
          changedAnything = true;
          error.hidden = true;
        } catch (err) {
          // Put the box back where the mailbox actually is, or it shows a tag
          // the message does not carry.
          box.checked = !box.checked;
          error.hidden = false;
          error.textContent = err instanceof Error ? err.message : String(err);
        }
      });
    }

    overlay.addEventListener("mousedown", (event) => {
      if (event.target === overlay) close();
    });
    overlay
      .querySelector<HTMLButtonElement>("button[data-act='close']")!
      .addEventListener("click", close);
    document.addEventListener("keydown", onKey, true);

    document.body.append(overlay);
    overlay.querySelector<HTMLButtonElement>("button[data-act='close']")!.focus();
  });
}
