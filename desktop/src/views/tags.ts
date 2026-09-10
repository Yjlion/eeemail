/**
 * Creating, colouring, renaming and deleting tags.
 *
 * The engine has carried a colour per label since migration 166 --
 * `create_label` takes one, `set_label_color` changes one, and the sidebar has
 * been drawing the dot all along. What did not exist was any way to make a
 * label: `get_labels` was the only one of the eight label methods this client
 * called. So this screen is entirely a frontend feature.
 *
 * Colour crosses the wire as an integer and comes back as a hex string, which
 * is not symmetry anyone would design, but it is what the RPC does: the
 * engine stores `0xRRGGBB` and `api/types/email.rs` renders it for display.
 *
 * System labels are not editable. `Archive`, `Trash` and `Unverified` are
 * stored as reserved labels whose *names* are the sync-wire identifiers, so
 * renaming one locally would silently stop matching what another device sends.
 */

import { rpc } from "../client";
import { state, changed } from "../state";
import { escapeHtml } from "../html";
import type { Label } from "../types";

/** The colour offered to a tag created without one being chosen. */
const DEFAULT_COLOR = "#2563eb";

/** `#rrggbb` to the `0xRRGGBB` integer the engine stores. */
function colorToInt(hex: string): number {
  return parseInt(hex.replace(/^#/, ""), 16);
}

/**
 * Re-reads the labels into `state`.
 *
 * `state.labels` is loaded once, at boot, and the sidebar draws from it. Every
 * write here has to refresh it or the tag that was just created does not appear
 * until the next launch.
 */
async function refreshLabels(): Promise<void> {
  state.labels = (await rpc.call("get_labels", [state.accountId])) as Label[];
}

export async function renderTags(el: HTMLElement): Promise<void> {
  await refreshLabels();
  const userLabels = state.labels.filter((l) => !l.isSystem);
  const systemLabels = state.labels.filter((l) => l.isSystem);

  const row = (l: Label) => `
    <div class="tag-row" data-label-id="${l.id}">
      <span class="dot" style="background:${escapeHtml(l.color ?? "transparent")}"></span>
      <input class="tag-name" value="${escapeHtml(l.name)}" aria-label="Tag name" />
      <input class="tag-color" type="color" value="${escapeHtml(l.color ?? DEFAULT_COLOR)}"
             aria-label="Tag colour" />
      <button data-act="clear-color" title="Remove the colour">No colour</button>
      <button data-act="save">Save</button>
      <button data-act="delete" class="danger">Delete</button>
    </div>`;

  el.innerHTML = `
    <div class="tags">
      <h1>Tags</h1>
      <p class="lede">
        A tag is a label you put on mail, and a message can carry as many as you
        like &mdash; they are not folders, so tagging one does not move it out of
        anywhere. Tags sync to your other devices by name.
      </p>

      <form id="new-tag" class="new-tag">
        <h2 class="section">New tag</h2>
        <div class="tag-row">
          <input name="name" placeholder="Name" required autocomplete="off" />
          <input name="color" type="color" value="${DEFAULT_COLOR}" aria-label="Colour" />
          <label class="check">
            <input name="nocolor" type="checkbox" />
            No colour
          </label>
          <button type="submit">Create</button>
        </div>
      </form>

      <h2 class="section">Your tags (${userLabels.length})</h2>
      ${userLabels.length ? userLabels.map(row).join("") : `<div class="empty small">None yet</div>`}

      <h2 class="section">Built in</h2>
      <p class="hint">
        These come with every mailbox and cannot be renamed or removed: their
        names are what your other devices match on.
      </p>
      <div class="tag-system">
        ${systemLabels.map((l) => `<span class="badge plain">${escapeHtml(l.name)}</span>`).join("")}
      </div>

      <div class="error" id="tags-error" hidden></div>
    </div>
  `;

  const error = el.querySelector<HTMLElement>("#tags-error")!;
  const guard = async (work: () => Promise<unknown>) => {
    error.hidden = true;
    error.className = "error";
    try {
      await work();
      await renderTags(el);
      // The sidebar draws the tag list, and it is not this element.
      changed();
    } catch (err) {
      error.hidden = false;
      error.textContent = err instanceof Error ? err.message : String(err);
    }
  };

  el.querySelector<HTMLFormElement>("#new-tag")?.addEventListener("submit", (event) => {
    event.preventDefault();
    const form = event.target as HTMLFormElement;
    const name = (form.elements.namedItem("name") as HTMLInputElement).value.trim();
    if (!name) return;
    const noColor = (form.elements.namedItem("nocolor") as HTMLInputElement).checked;
    const color = (form.elements.namedItem("color") as HTMLInputElement).value;
    void guard(() =>
      rpc.call("create_label", [state.accountId, name, noColor ? null : colorToInt(color)]),
    );
  });

  for (const rowEl of el.querySelectorAll<HTMLElement>(".tag-row[data-label-id]")) {
    const id = Number(rowEl.dataset["labelId"]);
    const nameInput = rowEl.querySelector<HTMLInputElement>(".tag-name")!;
    const colorInput = rowEl.querySelector<HTMLInputElement>(".tag-color")!;
    const original = state.labels.find((l) => l.id === id);

    rowEl.querySelector<HTMLButtonElement>("[data-act='save']")?.addEventListener("click", () => {
      const name = nameInput.value.trim();
      if (!name) return;
      void guard(async () => {
        // Two calls, and only the ones that changed. Renaming a label to its
        // own name is a sync message to every other device saying nothing.
        if (name !== original?.name) {
          await rpc.call("rename_label", [state.accountId, id, name]);
        }
        if (colorInput.value !== original?.color) {
          await rpc.call("set_label_color", [state.accountId, id, colorToInt(colorInput.value)]);
        }
      });
    });

    rowEl
      .querySelector<HTMLButtonElement>("[data-act='clear-color']")
      ?.addEventListener("click", () => {
        void guard(() => rpc.call("set_label_color", [state.accountId, id, null]));
      });

    rowEl.querySelector<HTMLButtonElement>("[data-act='delete']")?.addEventListener("click", () => {
      if (!window.confirm(`Delete the tag "${original?.name ?? ""}"? The mail it is on stays.`)) {
        return;
      }
      void guard(async () => {
        await rpc.call("delete_label", [state.accountId, id]);
        // The deleted tag may be the view that is open behind this screen.
        if (state.view.kind === "label" && state.view.labelId === id) {
          state.view = { kind: "tag", tag: "inbox" };
        }
      });
    });
  }
}
