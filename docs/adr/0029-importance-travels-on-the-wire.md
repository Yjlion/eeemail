# 0029 — Importance travels on the wire, and is absent when normal

**Status:** Accepted — 2026-09-09

## Context

Email has carried a sender's claim about how important a message is since the
eighties, and carries it three different ways because nobody agreed on one:

- `Importance: high | normal | low` — RFC 2156, registered by RFC 4021. What
  Outlook writes and reads.
- `X-Priority: 1 (Highest)` … `5 (Lowest)` — no RFC at all, and implemented
  approximately everywhere. Thunderbird's.
- `Priority: urgent | normal | non-urgent` — RFC 2156 again, and about
  *delivery handling* rather than about how the reader should feel.

The engine underneath eeemail knows about none of them. It is a chat engine:
messages in a conversation do not have priorities, so there is no field to put
one in and no code that reads one. A message arriving from Outlook marked urgent
looked exactly like every other message, and there was no way to mark one going
the other way.

The alternative to putting this on the wire was a reserved label, like
`Archive`, `Trash` and `Unverified` — no upstream patches, and it would sync
between the user's own devices through machinery that already exists. It was
rejected because it answers a different question. A local flag is a note to
yourself; `Importance:` is something a correspondent said about their own
message, and something the user can say to a correspondent. Half of the feature
is interoperating with the mail clients everybody else is using.

## Decision

**eeemail writes `Importance:` and `X-Priority:`, and reads all three.**

Writing both, because clients read different ones and writing only one makes
the mark invisible to about half of everyone. Reading all three, because
receiving is where being liberal costs nothing.

**`Priority:` is read and never written.** It is about what relays should do,
not about what a reader should think, and writing it would be asking the
infrastructure for something rather than telling a person something.

**Precedence on receipt is `Importance`, then `X-Priority`, then `Priority`** —
ordered by how deliberate each one is. `Importance` is what a client sets when
the user ticks a box; `X-Priority` is frequently set by a mailer on the user's
behalf; `Priority` is about delivery.

**An unrecognised value does not stop the search.** Parsing returns `None` for
a word it does not know, which means "this header said nothing, try the next
one", as distinct from a header that explicitly said `normal`, which stops. A
message with `Importance: garbage` and `X-Priority: 1` is high.

**A normal message stores no row and emits no header.** This is the load-bearing
half of the decision. Upstream has tests that compare rendered MIME
byte-for-byte; a header on every message would move all of them, and carrying
those patches forever is exactly the trade
[ADR 0012](0012-rpc-and-cli.md) refuses elsewhere. So `msg_importance` holds
only the messages somebody marked, and `MimeFactory` emits nothing when the
value is `Normal`.

**Storage is a side table, not `msgs.param`.** `param` is a serialised blob, and
the message list needs this per row — a side table joins once for a list where a
param would be parsed per message.

## Consequences

**`send_email` grew a seventh positional parameter, and that broke every
caller.** `yerpc` compares positional arity with `!=`, so a six-argument call to
a seven-parameter method is `invalid params`, not a `None` for the missing one.
`scripts/e2e-pass.py`, `scripts/interop-pass.py` and
`scripts/gpg-interop-pass.py` all call it and **no CI job runs any of them**, so
nothing would have caught a stale call. All eight call sites across the three
scripts were swept in the same change. Any future signature change owes the same
sweep; the ledger and the method's own doc comment now say so.

**Marking a message you already sent changes only your copy.** The headers went
out with it and nothing here rewrites what the correspondent received. The
context-menu item is honest about this in its code comment and silent about it
on screen, which is a small dishonesty worth revisiting.

**Importance is not synced between the user's own devices.** It rides the
message on the wire, so a correspondent sees it; but a mark the user applies
afterwards is local. The reserved-label design would have got this for free.
Recorded rather than hidden.

**A sender's claim is displayed as a claim.** The badge says what the message
says about itself. Nothing here weighs it, sorts by it, or lets it change where
a message goes — an "important" flag that moved mail would be a filter written
by the sender.
