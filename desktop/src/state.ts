/**
 * What the app is currently showing, and the account it is showing it from.
 *
 * A module-level object rather than a store library: the whole UI is a handful
 * of views over one account, and a framework's worth of machinery to hold six
 * fields would be more code than the fields.
 */

import type { Label, SystemTag } from "./types";

/** Which list the middle pane is showing. */
export type View =
  | { kind: "tag"; tag: SystemTag }
  | { kind: "label"; labelId: number; name: string }
  | { kind: "search"; query: string };

/** Which full-pane screen is up, if any. `null` means the normal three panes. */
export type Screen = "setup" | "composer" | "contacts" | "settings" | null;

export const state = {
  accountId: 0,
  labels: [] as Label[],
  view: { kind: "tag", tag: "inbox" } as View,
  screen: null as Screen,
  messageIds: [] as number[],
  selectedMsgId: null as number | null,
  /** Prefilled when the composer is opened as a reply. */
  composerDraft: null as null | {
    to: string;
    cc: string;
    bcc: string;
    subject: string;
    body: string;
  },
  /** How many messages are waiting in the unverified view, for the sidebar badge. */
  unverifiedCount: 0,
};

/** Views re-render through this, so no view needs a reference to another. */
type Listener = () => void;
const listeners = new Set<Listener>();

export function onChange(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function changed(): void {
  for (const listener of listeners) listener();
}

/**
 * Reads the view out of `location.hash`.
 *
 * `#/tag/inbox`, `#/tag/inbox/101`, `#/label/10`, `#/screen/composer`.
 *
 * `#/first-run` is deliberately not one of them: it changes no state, so it
 * falls through to the default view and the disclosure dialog opens over it.
 * See `shell.ts`.
 *
 * Deep links exist mostly so `scripts/screenshots.sh` can photograph a screen
 * without driving clicks, which is what keeps the images reproducible. They are
 * also the cheapest possible back/forward, so they earn their keep twice.
 */
export function applyHash(): boolean {
  const parts = location.hash.replace(/^#\/?/, "").split("/").filter(Boolean);
  if (parts.length === 0) return false;

  if (parts[0] === "screen" && parts[1]) {
    state.screen = parts[1] as Screen;
    return true;
  }
  if (parts[0] === "tag" && parts[1]) {
    state.screen = null;
    state.view = { kind: "tag", tag: parts[1] as SystemTag };
    state.selectedMsgId = parts[2] ? Number(parts[2]) : null;
    return true;
  }
  if (parts[0] === "label" && parts[1]) {
    state.screen = null;
    const labelId = Number(parts[1]);
    const label = state.labels.find((l) => l.id === labelId);
    state.view = { kind: "label", labelId, name: label?.name ?? "" };
    state.selectedMsgId = parts[2] ? Number(parts[2]) : null;
    return true;
  }
  return false;
}
