# 0027 — A blocklist that trashes on arrival

**Status:** Accepted — 2026-09-08

## Context

Core already has a notion of a blocked contact. `Contact::block` sets
`contacts.blocked`, `get_all_blocked` lists them, and both are JSON-RPC methods.
`email::search` already filters them out of results.

**None of it stops mail.** A blocked sender's message is fetched, decrypted and
stored exactly like any other; `receive_imf.rs` computes `from_id_blocked` in
`from_field_to_contact_id` and the caller discards it. The only consequence is
that the message's chat is a blocked chat, which marks the message read. For a
messenger that is coherent — the contact request is the feature, and a blocked
person's messages sit in a place you do not look. For an email client, "blocked"
that leaves mail in the mailbox is a promise the user can watch being broken.

The second gap is scope. `contacts.blocked` is a column on a contact row, so it
can only describe somebody who already has one. The thing people actually want a
blocklist for is the sender who has *not* written yet, or the domain that sends
from a fresh local-part every week. A blocklist that requires the correspondent
to exist first cannot express either.

## Decision

**A blocked sender's mail is moved to `Trash` on arrival, and waits there.**

`email::blocklist::apply` runs from the existing best-effort block at the single
success exit of `receive_imf_inner` — the same hook `rawmime`, `recipients`,
`threading`, `labels` and `gating` already use, so this adds no new patch site
to an upstream file. It runs **after** `gating::apply` and so has the last word:
gating decides whether a stranger's mail waits in `Unverified`, and this decides
whether it belongs in the mailbox at all. A blocked sender who is also a
stranger ends up in `Trash`, which is what blocking them meant.

**Trashed, not destroyed.** `Trash` is the only place in eeemail that destroys
mail and it waits first ([ADR 0019](0019-recoverable-ephemeral-expiry.md)); a
blocked message takes the same `TrashPurgeDays` deadline as everything else, and
the reading pane says why it is there. Destroying on arrival was the
alternative and is worse in the case that decides it: a pattern typed with a
typo, or a domain blocked more broadly than intended, silently eats mail that
nobody can then discover was missing.

**A pattern is an address or a domain.** `spam@example.com` matches that
address; `@example.com` matches every address in exactly that domain. Both
case-insensitively, in their own `blocklist` table (migration 172), keyed by a
normalised form for the same reason `labels.name_norm` is — a list holding both
`Spam@Example.com` and `spam@example.com` is a list with a hole in it.

**A domain pattern does not match subdomains.** `@example.com` leaves
`mail.example.com` alone. This is narrower than some people will expect and it
is deliberate: a subdomain rule cannot be undone selectively, so blocking
`@example.com` to stop one marketing subdomain would also block the person's
actual mail. A blocklist whose reach the user cannot predict is one they cannot
use confidently on their own mailbox.

**A pattern that could never match an address is refused, not stored.** An entry
that silently never fires is one the user believes is protecting them.

**Blocking a contact does both halves, in one call.** `blocklist::block_contact`
sets `contacts.blocked` *and* writes the blocklist row, and the RPC is
`block_sender` rather than a second call beside upstream's `block_contact`. The
contact row is what core's own filtering keys off; the blocklist row is the only
half that stops mail. A caller that did one of them would have a feature that
looks like it works. `docs/handoff.md` records the `create_contact` /
`release_held_contact` pairing as exactly this trap, found in production; this
one is written as a single function so it cannot be half-done a third time.

## Consequences

**Blocked mail is still downloaded, decrypted and stored.** Refusing at IMAP
fetch time was considered and rejected: it leaves the message on the server,
which contradicts [ADR 0003](0003-imap-as-transport.md), and it makes "let me
see what I blocked" unanswerable. Blocking saves attention, not bandwidth, and
the docs should not imply otherwise.

**Blocking is not retroactive.** Mail already received stays where it is. The
user has already been shown it; retroactively binning it would be a surprise
with no undo, and the trash is a place they can put it themselves.

**Unblocking is not retroactive either.** Removing a pattern lets the next
message through and does not restore what was already trashed.

**The decision is never synced.** Every device holds the same blocklist and
reaches the same conclusion about the same message, so pushing a `Trash` label
for it would only race the local decision — the same reasoning as the unverified
sweep. What syncs is the *list*, if and when the blocklist is added to device
sync; today it is per-device, and that is a gap rather than a decision.

**`Reason::Blocked` is a fourth trash reason**, which the reading pane, the
JSON-RPC `TrashReason` and `desktop/src/types.ts` all had to learn. Adding a
fifth means the same four places.

**A blocked contact disappears from the contacts list.** `get_contacts` filters
`blocked=0` upstream, so the address book cannot show them and the blocklist
section is where they are managed instead. Unblocking is therefore done from the
blocklist, not from the contact — which is the right place, but it means the two
controls for one person live on different parts of the screen.
