/**
 * The context menu.
 *
 * One implementation, opened from the message list and from the reading pane,
 * because a menu that disagrees with itself about what a message can do is
 * worse than no menu. What goes *in* it is decided by `actions.ts`; this module
 * only knows how to show a list and report which item was picked.
 *
 * A DOM-built menu rather than a native one: a native menu means a Tauri menu
 * plugin, an ACL entry and a second place where the item list lives. The
 * trade-off is that this has to do its own dismissal and its own keyboard
 * handling, which is the code below.
 */

/** One row of the menu. `null` is a separator. */
export type MenuItem =
  | { label: string; act: string; disabled?: boolean; danger?: boolean }
  | null;

/** The menu currently open, so a second right-click replaces rather than stacks. */
let open: { el: HTMLElement; close: (act: string | null) => void } | null = null;

/** Closes whatever is open, reporting nothing picked. */
export function closeMenu(): void {
  open?.close(null);
}

/**
 * Opens a menu at viewport coordinates and resolves with the chosen `act`, or
 * `null` if it was dismissed.
 *
 * Dismissed by Escape, by a click anywhere else, by a scroll, and by the window
 * losing focus. All four, because a menu that survives one of them ends up
 * floating over an unrelated screen.
 */
export function openMenu(x: number, y: number, items: MenuItem[]): Promise<string | null> {
  closeMenu();

  return new Promise((resolve) => {
    const el = document.createElement("div");
    el.className = "menu";
    el.setAttribute("role", "menu");

    for (const item of items) {
      if (item === null) {
        const rule = document.createElement("hr");
        el.append(rule);
        continue;
      }
      const button = document.createElement("button");
      button.type = "button";
      button.setAttribute("role", "menuitem");
      // `textContent`, not `innerHTML`: a label can carry a sender's display
      // name, and this is the app document.
      button.textContent = item.label;
      button.disabled = item.disabled === true;
      if (item.danger) button.classList.add("danger");
      button.addEventListener("click", () => close(item.act));
      el.append(button);
    }

    document.body.append(el);

    // Positioned after insertion, because until it is in the document it has no
    // measurable size and a menu opened near the right edge would hang off it.
    const box = el.getBoundingClientRect();
    const left = Math.max(4, Math.min(x, window.innerWidth - box.width - 4));
    const top = Math.max(4, Math.min(y, window.innerHeight - box.height - 4));
    el.style.left = `${left}px`;
    el.style.top = `${top}px`;

    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        close(null);
      }
    };
    const onPointer = (event: MouseEvent) => {
      if (!el.contains(event.target as Node)) close(null);
    };
    const onAway = () => close(null);

    function close(act: string | null): void {
      if (open?.el !== el) return;
      open = null;
      el.remove();
      document.removeEventListener("keydown", onKey, true);
      document.removeEventListener("mousedown", onPointer, true);
      window.removeEventListener("blur", onAway);
      window.removeEventListener("scroll", onAway, true);
      resolve(act);
    }

    document.addEventListener("keydown", onKey, true);
    // `mousedown` rather than `click`, so a press that starts outside dismisses
    // before it can activate whatever it landed on.
    document.addEventListener("mousedown", onPointer, true);
    window.addEventListener("blur", onAway);
    window.addEventListener("scroll", onAway, true);

    open = { el, close };
    el.querySelector<HTMLButtonElement>("button:not([disabled])")?.focus();
  });
}
