# 0008 — Recipient sets and threading are per-message, and threading never merges by subject

**Status:** Accepted — 2026-08-31

## Context

Core's message model is built around the *chat*. Who receives a message follows
from chat membership, and which messages belong together follows from which chat
they were assigned to. Both are the right answer for a chat app and the wrong
one for an email client:

- **Recipients.** Two messages in one conversation routinely have different `To`
  and `Cc` lists — someone is added, someone is dropped, someone replies only to
  the sender. Reply-all is only possible if each message remembers what *it* was
  addressed to. Core keeps `msgs.to_id`, a single contact, and
  `MimeMessage::recipients`, which concatenates `To` and `Cc` because for group
  membership the distinction does not matter.
- **Grouping.** Email conversations are defined by `References` and
  `In-Reply-To`, not by sender or membership. Core already stores
  `mime_in_reply_to` and `mime_references` on every message but never groups by
  them.

Subject is the exception: `msgs.subject` already exists, is populated on
receive, and `Message::set_subject` already reaches the wire. It needed
verification, not implementation.

## Decision

**Recipient sets are a first-class per-message property**, in `msg_recipients`,
with `To`, `Cc` and `Bcc` kept apart and header order preserved. They are read
from `MimeMessage::merge_headers`, which has already resolved RFC 9788 protected
headers, and never from the raw bytes.

**`Bcc` is only ever written by the send path.** A `Bcc` header on a received
message either cannot exist or was added by an intermediary; trusting it would
let a sender make a recipient believe someone else was blind-copied.

**Threading follows reference chains only.** A thread is identified by the set of
Message-IDs it is known by — its members' own IDs and every ID they reference,
held in `thread_refs`. A message matching no thread starts one, a message
matching one joins it, and a message matching several merges them.

**JWZ step 5, subject-based merging, is not implemented.** It is the step that
produces false merges: every unrelated "Hi" or "Question" collapses into one
conversation. It is worse here than in a typical client, because encrypted mail
routinely carries a generic subject so as not to leak the real one, so the
subject is frequently *identical* across unrelated conversations.

## Consequences

- Grouping is independent of arrival order. A reply fetched before its parent
  starts a thread that the parent later joins; a message that references two
  existing fragments merges them. The oldest thread survives a merge, so a
  long-running conversation keeps its identity.
- Grouping can leave a conversation **split** when a correspondent's client
  fails to set `References`. This is the accepted cost: a split thread is a
  visible annoyance the user can work around, whereas a false merge silently
  shows two people each other's unrelated mail. If splits prove common in
  practice, the remedy is a narrow heuristic (same participants, same subject,
  close in time), not JWZ step 5.
- Thread *shape* — the parent/child tree — is derived on demand rather than
  stored, from `mime_in_reply_to` and `mime_references` on the messages
  themselves. Only the grouping is persisted, so there is one source of truth
  and nothing to keep in sync.
- A referenced message that was never received contributes structure but is not
  displayed. Its replies attach to the nearest ancestor actually held, and if
  there is none they become roots of the thread. This is JWZ's "prune empty
  containers" without materializing the containers.
- Tree building is iterative, and a message may only descend from an older one.
  Reply depth and `References` contents are attacker-influenced; recursion would
  risk a stack overflow and an unchecked parent map would risk a cycle.
- Anything reading `msgs` from our layer must guard its text columns with
  `IFNULL`. `mime_in_reply_to` and `mime_references` are not declared `NOT
  NULL` and are genuinely left NULL by some paths -- messages created by
  SecureJoin, for one. `rfc724_mid` and `subject` are guarded too, defensively:
  a NULL there is unreachable today only because core's own
  `Message::load_from_db` would fail on it first. This applies equally to the
  phases that follow, which key off the same table.
- Sending *to* an arbitrary recipient set — a `Cc` naming someone outside the
  conversation — is **not** part of this decision. It requires resolving
  encryption keys for addresses that have none, which is
  [0006](0006-encryption-policy.md)'s subject, and lands with it.
