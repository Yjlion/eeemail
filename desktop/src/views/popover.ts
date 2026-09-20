/**
 * A menu that drops down from a toolbar button.
 *
 * One open at a time, closed by a click anywhere else, by Escape, or by
 * whatever it was opened to do. Built for the composer, where every item has to
 * act without taking the caret out of the editor: items are pressed on
 * `mousedown` with the default prevented, the same way the toolbar buttons are,
 * so the selection a colour or font should land on is still there when it does.
 */

let open: { panel: HTMLElement; anchor: HTMLElement; dispose: () => void } | null = null;

/** Closes whichever popover is open. */
export function closePopover(): void {
  open?.dispose();
}

/**
 * Opens `panel` under `anchor`, or closes it if that anchor's popover is the
 * one already open, so the button that opened a menu also closes it.
 */
export function togglePopover(anchor: HTMLElement, build: () => HTMLElement): void {
  if (open?.anchor === anchor) {
    closePopover();
    return;
  }
  closePopover();

  const panel = build();
  panel.classList.add("popover");
  panel.setAttribute("role", panel.getAttribute("role") ?? "menu");
  // Positioned against the anchor's offset parent rather than the viewport,
  // so it scrolls with the composer instead of floating over the page.
  const host = anchor.offsetParent instanceof HTMLElement ? anchor.offsetParent : document.body;
  host.append(panel);
  panel.style.left = `${anchor.offsetLeft}px`;
  panel.style.top = `${anchor.offsetTop + anchor.offsetHeight + 4}px`;
  // Kept inside the host: a menu near the right edge opens leftwards rather
  // than running off the screen.
  const overflow = panel.offsetLeft + panel.offsetWidth - host.clientWidth;
  if (overflow > 0) panel.style.left = `${Math.max(0, panel.offsetLeft - overflow)}px`;
  anchor.setAttribute("aria-expanded", "true");

  const onDown = (event: MouseEvent) => {
    const target = event.target as Node;
    if (!panel.contains(target) && !anchor.contains(target)) closePopover();
  };
  const onKey = (event: KeyboardEvent) => {
    if (event.key === "Escape") closePopover();
  };
  document.addEventListener("mousedown", onDown, true);
  document.addEventListener("keydown", onKey, true);

  const current = {
    panel,
    anchor,
    dispose: () => {
      document.removeEventListener("mousedown", onDown, true);
      document.removeEventListener("keydown", onKey, true);
      panel.remove();
      anchor.setAttribute("aria-expanded", "false");
      if (open === current) open = null;
    },
  };
  open = current;
}

/**
 * Makes `el` act on press without taking focus.
 *
 * `mousedown` rather than `click`: a click moves focus out of the editor
 * first, and the selection the command should act on goes with it.
 */
export function onPress(el: HTMLElement, run: () => void): void {
  el.addEventListener("mousedown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    run();
  });
  // The keyboard route, which has no mousedown. Focus has already left the
  // editor by then, and Squire acts on the selection it kept.
  el.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      run();
    }
  });
}
