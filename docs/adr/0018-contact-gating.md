# 0018 — Mail from strangers is held, not delivered, and expires if never accepted

**Status:** Accepted — 2026-09-01 · Amended 2026-09-03 (the deadline moves
mail to Trash rather than discarding it; the window is a setting; the tag is now
called `Unverified`)

## Context

An inbox that anyone can write to is an inbox anyone can spend your attention
from. Spam filtering answers this statistically, badly, and at the cost of
reading everyone's mail on a server — which is not available to us, because the
server never sees plaintext ([0003](0003-imap-as-transport.md)).

Core already has the shape of an answer. Delta Chat puts a chat from an unknown
correspondent into `Blocked::Request` — visible, but requiring an explicit
accept before it becomes an ordinary conversation and before read receipts are
sent to it. Core also already knows, per contact, both things worth knowing
about identity here: `Contact::is_verified` (SecureJoin completed, so the key
survived an active attacker) and `Origin::is_known` (you chose to know them,
rather than they mailed you once).

Those two predicates are the *same pair* [0011](0011-receipts-and-ephemeral.md)
composed for the verified-only read-receipt policy. Whatever we build here
should use them, not a third notion of "do I know this person".

## Decision

**Incoming mail from a sender who is neither verified nor known gets the
`Holding` system tag instead of reaching the Inbox.** On by default for eeemail
accounts, applied at setup by `email::policy::apply_defaults` rather than as a
compile-time default — the same arrangement `ForceEncryption` has, and for the
same reason ([0012](0012-rpc-and-cli.md)): flipping the compile-time default
breaks upstream tests that assert a stranger's mail reaches the inbox, and
carrying those patches forever is a poor trade for a value written once.

**Held mail is purged 30 days after it arrives.** Not archived — discarded.

> **Amended 2026-09-03.** It is **swept into `Trash`**, not discarded, and the
> window is `Config::UnverifiedTrashDays` rather than a constant. Two reasons.
> The deadline a few lines away in [0019](0019-recoverable-ephemeral-expiry.md)
> had already grown a recoverable window on the argument that a timer must not
> destroy the only copy of a mailbox, and that argument does not stop applying
> because the mail came from a stranger — it was the *same deadline problem*,
> answered two different ways in two adjacent modules. And "30 days" was a
> compile-time constant the UI could only report, so a user who found the window
> too short or too long had nothing to do about it.
>
> `Trash` then applies its own deadline and destroys, which makes it the single
> place in eeemail where mail is destroyed on a timer. The consequences below
> are unchanged in substance: mail from someone you never accepted still goes
> away, it now takes two windows rather than one and is recoverable throughout
> the second.
>
> **The window is measured from `held_at` and read afresh on every sweep**, so
> changing it moves mail that is already waiting. A deadline stored per row at
> hold time would have been a second source of truth that silently outvoted the
> setting, so `held_msgs.purge_at` was dropped in migration 171.
>
> **`0` means never sweep**, not sweep at once. Someone who wants unverified
> mail gone immediately turns gating off, which releases it to the inbox where
> they can delete it.

**Accepting or verifying a contact releases their mail**, the messages already
held as well as everything after.

Gating is a setting and can be turned off, in which case everything reaches the
Inbox as before.

## Consequences

- **This can lose mail, and that is the trade.** A message from someone you
  genuinely wanted to hear from, whom you never accepted, is gone in 30 days —
  since the 2026-09-03 amendment, gone *to Trash*, and gone for real 30 days
  after that.
  The alternative — hold forever — turns the view into a second inbox that
  accumulates exactly the mail nobody wanted to look at, which is the problem
  this is meant to solve. It is visible, sorted and searchable, and 30 days
  is longer than the window in which unsolicited mail is worth anything.
- **Accepting a sender does not reach mail that has already been swept.**
  `release` reads `held_msgs`, and the sweep deletes that row. The message is in
  `Trash`, visible and restorable by hand. Releasing mail back out of a bin the
  user may have deliberately emptied would be the stranger behaviour, and the
  settings copy says which it is.
- **On by default is the point.** A gate the user has to find and enable
  protects the people who already knew to look for it. This is the same reason
  the release notes called out ephemeral shipping disabled as a problem: a
  feature that ships off is a feature that does not exist.
- Purging is a **local** decision and is not synced. A device that has been
  offline for six months must not come back and purge mail another device is
  still holding, and a device that just installed must not inherit deadlines it
  cannot reason about. Each device holds and purges on its own clock; accepting
  a contact syncs, and that is the action that actually matters.
- Held mail is **downloaded and decrypted normally**. Holding is a view, not a
  refusal to fetch: refusing would leave the message on the server, contradicting
  [0003](0003-imap-as-transport.md), and would make "let me see what this is"
  impossible to answer.
- Read receipts are already not sent for contact requests, and the verified-only
  policy already declines to send to unknown contacts. Gating adds no new
  disclosure — a held sender learns nothing about whether you read their mail,
  which is the correct behaviour and falls out of what is already there.
- Reusing `Blocked::Request` rather than inventing a parallel state means a
  contact accepted in one part of the client is accepted everywhere, including
  in code paths eeemail did not write.
- Mail from a contact you have written to is never held: sending to someone sets
  their origin to known, so the first reply arrives in the Inbox. Gating asks
  "did you choose this person?", not "have they proved themselves?".
