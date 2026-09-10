/**
 * Several accounts in one window.
 *
 * The engine has had multiple accounts all along -- `select_account`,
 * `get_all_accounts` and the rest have existed since before this client did.
 * What the client did was `state.accountId = ids[0]`, once, at boot. So a
 * second profile was reachable only by not having a first one.
 *
 * The whole difficulty here is that `state.accountId` is not the only thing
 * that belongs to an account. Labels, the unverified count, the selected
 * message, the list itself and the contacts view's module-level cache are all
 * per-account, and every one of them survives a change to `accountId` unless
 * something clears it. [`switchAccount`] is that something, and it is the only
 * supported way to change accounts for exactly that reason.
 *
 * See `docs/adr/0026-several-accounts-in-one-window.md`.
 */

import { rpc } from "./client";
import { state } from "./state";
import { reload, refreshConnectivity } from "./nav";
import { resetContactsCache } from "./views/contacts";
import type { Account, Label } from "./types";

/** Re-reads the account list. Cheap, and the sidebar draws from it. */
export async function loadAccounts(): Promise<void> {
  state.accounts = (await rpc.call("get_all_accounts")) as Account[];
}

/**
 * Prepares one account for use: eeemail's defaults, then its IO loop.
 *
 * The order is load-bearing and is the same order `setup.ts` uses.
 * `policy::apply_defaults` refuses to touch a configured account, so on an
 * account that is already set up this is a no-op that costs one round trip --
 * which is the point: it is safe to call on every account at every boot, and
 * an account that was created but never finished still gets its defaults.
 *
 * `start_io` on *every* account, not only the visible one, because mail that
 * arrives for a profile nobody is looking at is still mail that has arrived.
 * The alternative -- start on switch, stop on leave -- means the other
 * accounts silently stop receiving, which is the behaviour of a chat app with
 * one conversation open, not of a mailbox.
 */
export async function prepareAccount(accountId: number): Promise<void> {
  await rpc.call("apply_eeemail_defaults", [accountId]);
  await rpc.call("start_io", [accountId]);
}

/**
 * Opens an account: clears what belonged to the last one, then loads this one.
 *
 * Everything reset here is per-account state that a bare `accountId` change
 * would leave pointing at the previous mailbox: a label id from account A means
 * a different label in account B, and a selected message id means a different
 * message or none.
 *
 * Unconditional, so it is also the path a freshly created account takes. The
 * first-run form used to set `accountId` and repaint, which left `state.labels`
 * empty and `state.messageIds` unfetched until the next launch.
 */
export async function openAccount(accountId: number): Promise<void> {
  // Persisted, so the next launch opens the account the user left open.
  await rpc.call("select_account", [accountId]);
  state.accountId = accountId;

  // Ids from the previous account address nothing in this one.
  state.selectedMsgId = null;
  state.selectedContactId = null;
  state.view = { kind: "tag", tag: "inbox" };
  state.unverifiedCount = 0;
  state.messageIds = [];
  state.connectivity = null;
  // Module-level, and so invisible to anything that only clears `state`.
  resetContactsCache();

  // Started at boot already, unless this account was created since.
  await prepareAccount(accountId);
  state.labels = (await rpc.call("get_labels", [accountId])) as Label[];
  void refreshConnectivity();
  // `reload`, not `changed`: the list belongs to the account, and repainting
  // without refetching draws the new account's heading over the old one's mail.
  await reload();
}

/** Shows a different account, or does nothing if it is already the open one. */
export async function switchAccount(accountId: number): Promise<void> {
  if (accountId === state.accountId) return;
  await openAccount(accountId);
}
