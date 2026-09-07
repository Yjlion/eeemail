/**
 * Re-reading the current view from the engine, and repainting.
 *
 * This lives in its own module rather than in `main.ts` because *everything*
 * that changes which messages should be on screen has to call it, and for two
 * releases only some things did. `sidebar.ts` set `state.view` and called
 * `changed()`, which repaints from `state.messageIds` -- so clicking Sent
 * painted the Sent heading over the previous view's rows. The screenshots
 * navigate by hash, and hash navigation went through `main.ts`'s copy of this,
 * so nothing caught it.
 *
 * The rule: change `state.view`, call [`reload`], not `changed`.
 */

import { rpc } from "./client";
import { state, changed } from "./state";
import { refreshList } from "./views/list";

/**
 * How many messages are waiting in the unverified view, for the sidebar badge.
 *
 * Here rather than in `sidebar.ts` so the sidebar only renders, and so nothing
 * that reloads a list has to remember to refresh the badge separately.
 */
export async function refreshUnverifiedCount(): Promise<void> {
  try {
    const ids = (await rpc.call("get_tagged_messages", [
      state.accountId,
      "unverified",
    ])) as number[];
    state.unverifiedCount = ids.length;
  } catch {
    // A count is decoration. Failing to fetch it must not take the sidebar down.
    state.unverifiedCount = 0;
  }
}

/** Re-reads the current list and the badge from the engine, then repaints. */
export async function reload(): Promise<void> {
  await Promise.all([refreshList(), refreshUnverifiedCount()]);
  changed();
}

/**
 * Asks the engine to look for new mail, then reloads.
 *
 * `maybe_network` is a nudge, not a fetch: it tells the scheduler the network
 * may have come back, and the IMAP loop does the work and emits `IncomingMsg`
 * when something arrives. So this reloads what is already known immediately and
 * lets the event stream deliver the rest, rather than pretending to wait for a
 * round trip whose length nobody can predict.
 */
export async function checkForNewMail(): Promise<void> {
  if (state.refreshing) return;
  state.refreshing = true;
  changed();
  try {
    await rpc.call("maybe_network");
    await reload();
  } finally {
    state.refreshing = false;
    await refreshConnectivity();
    changed();
  }
}

/** Reads the engine's connectivity for the indicator beside the refresh button. */
export async function refreshConnectivity(): Promise<void> {
  try {
    state.connectivity = (await rpc.call("get_connectivity", [
      state.accountId,
    ])) as number;
  } catch {
    state.connectivity = null;
  }
}
