/**
 * What the app is currently showing, and the account it is showing it from.
 *
 * A module-level object rather than a store library: the whole UI is a handful
 * of views over one account, and a framework's worth of machinery to hold six
 * fields would be more code than the fields.
 */

import type { Account, Label, SystemTag } from "./types";

/** Which list the middle pane is showing. */
export type View =
  | { kind: "tag"; tag: SystemTag }
  | { kind: "label"; labelId: number; name: string }
  | { kind: "search"; query: string };

/** Which full-pane screen is up, if any. `null` means the normal three panes. */
export type Screen =
  | "setup"
  | "composer"
  | "contacts"
  | "settings"
  | "tags"
  | null;

export const state = {
  accountId: 0,
  /**
   * Every profile the engine knows about, configured or not.
   *
   * Held here rather than fetched per render because the sidebar draws it on
   * every repaint, and because `accountId` is only meaningful against a list
   * that agrees with it.
   */
  accounts: [] as Account[],
  /**
   * Which account the setup form is configuring, or `null` to create one.
   *
   * This exists because the form used to read `state.accountId` directly, which
   * meant "add another account" would have reconfigured the account already
   * open. `null` is the request to make a new one; a number is a setup being
   * resumed.
   */
  setupAccountId: null as number | null,
  labels: [] as Label[],
  view: { kind: "tag", tag: "inbox" } as View,
  screen: null as Screen,
  messageIds: [] as number[],
  selectedMsgId: null as number | null,
  /**
   * The message being written. Prefilled when the composer is opened as a
   * reply, and kept up to date as the user types, so a repaint puts the
   * composer back as it was rather than as it was opened.
   */
  composerDraft: null as null | ComposerDraft,
  /** Which contact the contacts screen has open, if any. */
  selectedContactId: null as number | null,
  /** How many messages are waiting in the unverified view, for the sidebar badge. */
  unverifiedCount: 0,
  /** A check for new mail is in flight, so the refresh control is busy. */
  refreshing: false,
  /**
   * The engine's last reported connectivity, or `null` before it has been
   * asked. One of core's `DC_CONNECTIVITY_*` values: 1000 not connected, 2000
   * connecting, 3000 working, 4000 connected.
   */
  connectivity: null as number | null,
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
 * `#/tag/inbox`, `#/tag/inbox/101`, `#/label/10`, `#/screen/composer`,
 * `#/screen/contacts/7`.
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
    // `#/screen/contacts/7` selects a contact, which is the only screen with
    // anything to select. Written here rather than in the view so a deep link
    // resolves before the first paint rather than after it.
    state.selectedContactId = parts[2] ? Number(parts[2]) : null;
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

/** Everything the composer needs to put itself back after a repaint. */
export type ComposerDraft = {
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  body: string;
  /** The formatted body, when the draft is in formatted mode. */
  html: string | null;
  /** Cc and Bcc rows opened by the user; a row with content is shown regardless. */
  showCc?: boolean;
  showBcc?: boolean;
  importance?: "high" | "normal" | "low";
  /**
   * The padlock, once the user has touched it. Unset means it follows whether
   * everyone has a key, which is what it should do until someone decides.
   */
  encrypt?: boolean;
  /**
   * The composer has placed the signature, so it must not place it again. Set
   * even if the user then deleted it: that was their decision, not a gap.
   */
  signed?: boolean;
  /** The one file this message carries. */
  attachment?: File | null;
};
