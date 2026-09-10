# 0026 — Several accounts in one window, all of them fetching

**Status:** Accepted — 2026-09-08

## Context

The engine has had multiple accounts since long before this client existed.
`Accounts` keeps a `BTreeMap<u32, Context>`, persists a `selected_account` in
`accounts.toml`, and exposes `add_account`, `remove_account`, `select_account`,
`get_selected_account_id` and `set_accounts_order` — every one of which is
already a JSON-RPC method the desktop shell can call, because the shell is one
untyped pipe with no allowlist ([ADR 0013](0013-desktop-ui.md)).

The client used none of it. `main.ts` read `get_all_account_ids`, took `ids[0]`,
and never mentioned accounts again. A second profile was reachable only by not
having a first one: the setup form was shown when the account list was empty,
and it configured `state.accountId || add_account()` — so with an account
already open it would have reconfigured *that* account rather than making a new
one. There was no entry point, and the one code path that looked like it might
be one was a trap.

So this is not a question of what the engine can do. It is a question of what
`state.accountId` means in a client that assumed there was only ever one.

## Decision

**One window, an account picker in the sidebar, and `openAccount` as the only
way to change accounts.**

The picker is drawn only when there is more than one account. A picker with a
single entry is a control that cannot do anything, taking the top of the sidebar
away from the mail.

**Everything per-account is cleared by one function.** `accounts.ts:openAccount`
calls `select_account`, then resets `state.labels`, `state.selectedMsgId`,
`state.selectedContactId`, `state.view`, `state.unverifiedCount`,
`state.messageIds`, `state.connectivity` and the module-level caches in
`views/contacts.ts`, before reloading. It is unconditional and it is what a
newly created account goes through too.

This is the substance of the decision. `accountId` was never the only thing that
belonged to an account: a label id from account A names a different label in
account B, a selected message id names a different message or none, and
`views/contacts.ts` held its contact list and QR code at module scope where
nothing that clears `state` can reach them. A switch that set `accountId` and
repainted would show one account's mail under another account's headings — which
is exactly the class of bug `docs/handoff.md` records under "Navigating the
sidebar never re-read the list", one release earlier and one field over.

**Every account fetches, not just the visible one.** `apply_eeemail_defaults`
and `start_io` run for each account at boot, in a loop, each wrapped so one
account's failure cannot take down the boot of the others.

The alternative — start the selected account, stop the previous one on switch —
is cheaper and wrong. Mail arriving for a profile nobody is looking at is still
mail that has arrived; a client that only receives for the conversation you have
open is a messenger, not a mailbox. Someone with a work account and a personal
account expects both to be current when they switch.

**`state.setupAccountId` decides what the setup form configures**: `null` means
create one, a number means resume one. The form no longer reads
`state.accountId` at all.

## Consequences

**`apply_eeemail_defaults` is now called once per account per boot**, where it
was once per boot. It is a no-op on a configured account by construction
([ADR 0012](0012-rpc-and-cli.md)) — it early-returns on `is_configured()` — so
the cost is one round trip each and the benefit is that an account created but
never finished still gets eeemail's policy rather than upstream's.

**Fixing the switch fixed first-run too.** The setup form used to set
`accountId` and repaint, which left `state.labels` empty and `state.messageIds`
unfetched until the next launch. That had been true since the form was written
and was invisible because nothing else could change accounts. It now goes
through `openAccount` like everything else.

**Anything cached at module scope in a view is now a bug waiting to happen.**
`resetContactsCache()` exists because `views/contacts.ts` had two such
variables. A third one added later, in any view, will survive a switch and show
the wrong account's data, and nothing will fail to compile. The rule is that
per-account state lives in `state`, or it is reset in `openAccount`.

**More connections, more battery.** Starting IO for every account is what the
decision buys and what it costs. A user with six accounts opens six IMAP
connections at launch.

**Removing an account has no UI.** `remove_account` exists on the engine and is
not offered here. Deleting a mailbox whose local database *is* the mailbox
([ADR 0004](0004-local-store-and-raw-mime.md)) deserves more than a menu item,
and it can wait for a screen that explains what is destroyed.

**An unconfigured account can appear in the picker.** `get_all_accounts` returns
both kinds, and one shows as "Unfinished setup". Hiding it would leave somebody
who abandoned a setup half-way with no way back to it.
