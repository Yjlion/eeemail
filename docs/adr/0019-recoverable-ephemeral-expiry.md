# 0019 — Ephemeral expiry moves a message to Trash for 30 days instead of destroying it

**Status:** Accepted — 2026-09-01 · Supersedes the ephemeral half of [0011](0011-receipts-and-ephemeral.md) · Amended 2026-09-03 (the window governs the whole
of `Trash`, and is renamed accordingly)

## Context

[0011](0011-receipts-and-ephemeral.md) shipped ephemeral messages **disabled**,
against the plan, for one reason: expiry is destructive. Core's
`delete_expired_messages` rewrites the message row into a tombstone in the trash
chat and drops the content. For Delta Chat that costs a chat message. For
eeemail the local store is the only durable copy of the mailbox
([0004](0004-local-store-and-raw-mime.md)), and the server is a spool with
nothing left to re-download, so a timer means the user's mail deletes itself and
cannot be got back.

That reasoning was about **irreversibility**, not about timers. A user who sets
an expiry is asking for the message to go away; they are not asking to be unable
to change their mind about it an hour later.

Nothing in the ephemeral protocol requires immediate destruction locally. The
timer is an agreement with the correspondent about the message's lifetime, and
we honour it by removing the message from view when it fires. What the local
client does with the bytes for a grace period afterwards is a local matter, and
it is the same question every mail client answers with a trash folder.

## Decision

**Expiry moves the message to `Trash` and schedules a purge 30 days later.**
Content stays readable and restorable for that window. At purge, the message is
destroyed for real, by the same path a manual delete uses.

The window is a setting, `Config::EphemeralTrashDays`, not a constant.

> **Amended 2026-09-03.** Renamed to `Config::TrashPurgeDays`, because it is no
> longer about ephemeral messages. [0018](0018-contact-gating.md)'s deadline now
> sweeps unaccepted mail into `Trash` as well, so all three routes in — thrown
> away by hand, expired by a timer, swept out of `Unverified` — leave on this
> one deadline. That makes `Trash` the single place in eeemail that destroys
> mail on a timer, which is worth being true and worth the setting being named
> for it.
>
> `Config` is stored under its snake_case name, so the rename would have
> orphaned every existing value and dropped those accounts back to the
> compile-time default of `0` — destroy immediately, the one value a user of
> this setting would never have chosen. Migration 171 carries it over, and a
> test asserts that it does.
>
> The trash reason gained a third value, `Unaccepted`, so the reading pane can
> say why a message the user never touched is in the bin.

It is applied at setup like the gating default above, because upstream's
ephemeral tests assert that a fired timer removes the message — and expressing it
as a setting is better anyway: **zero means destroy immediately**, which is what
a user who wants a fired timer to mean *gone* would choose, and they can now say
so.

**The user can change their mind about the timer**, per message: re-time it,
clear it, or take the `Trash` tag off a message that has already expired.

**The global default stays `0`.** Ephemeral remains something the user turns on.

**Revisited 2026-09-02 (issue #3) and unchanged.** The question was put again
now that expiry is recoverable, on the grounds that the original objection --
that a timer silently destroys the only copy of the mailbox -- had been removed.
It has been, and that is not enough on its own: it disposes of an argument
against a non-zero default without supplying one for it. A duration would have
to be chosen, no duration is right for everyone's mail, and mail is not chat --
the correspondence people expect to still have in five years goes through the
same inbox as the message they would rather vanished. The machinery is complete
and every test covers the on case, so a user who wants this has one config value
to set.

## Consequences

- The conflict with [0004](0004-local-store-and-raw-mime.md) is resolved rather
  than accepted. Decrypted content is still canonical, and it now has a
  recoverable window before it stops being permanent — the property a mailbox
  needs and a chat does not.
- **The default is still off, and that is a separate judgement.** Recoverability
  removes the argument that a timer silently destroys mail; it does not create a
  positive argument that every conversation should start on a countdown. Whether
  mail expires is the user's call, and there is still no duration that is
  obviously right for everyone's mail.
- A 30-day window means an expired message is still on disk for 30 days. Anyone
  who set a timer expecting the bytes gone at the stroke of expiry does not get
  that. The tag is honest about where the message is, a user who wants immediate
  destruction can empty the trash, and one who wants it never to happen at all
  can set the window to zero.
- **Only the local copy is deferred.** Removal from the server and from the
  correspondent's client is unchanged and immediate; nothing about the agreement
  with the other side is weakened.
- The purge deadline is local and not synced, for the same reason
  [0018](0018-contact-gating.md)'s is: a device's own clock is the only one it
  can reason about.
- This costs one narrow patch to upstream's `delete_expired_messages`, which
  diverts a first-time expiry and leaves everything else alone. It is recorded
  in `docs/fork-patches.md`, and it is the kind of hook a future merge will
  surface loudly rather than break silently.
- Per-contact overrides still compose to the **shortest**, and the default is
  still applied on the first message to a conversation so that turning it off
  sticks. [0011](0011-receipts-and-ephemeral.md) is unchanged on both points,
  and entirely unchanged on read receipts.
