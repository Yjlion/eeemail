# 0009 — Archive is the presence of a label, and search is a separate entry point

**Status:** Accepted — 2026-08-31 · Amends [0005](0005-labels-not-folders.md)

## Context

[0005](0005-labels-not-folders.md) settled the model: labels plus archive, no
folder tree. Implementing it raised two questions it did not answer.

**Which way round is archive?** 0005 says "archive is simply the removal of the
Inbox label", which requires every message to be given an Inbox label as it
arrives, and the inbox to be a join. The alternative is the inverse: archive is
the *presence* of a reserved `Archive` label, and the inbox is what has not been
archived.

**Where does extended search live?** Core's `Context::search_msgs` matches the
message body only. An email client must also find a message by subject, by who
it was addressed to, and within a label.

## Decision

**Archive is the presence of a reserved `Archive` label.** The inbox needs no
rows at all: a message nothing has touched is in it.

**Extended search is a new entry point, `email::search::search`,** leaving
`Context::search_msgs` untouched.

**Search uses `LIKE`, not FTS.**

**Sync carries one `SyncData` variant, `EmailLabel`,** wrapping an enum defined
in our own module. Archiving is not a variant of its own; it is `Apply` or
`Unapply` of the reserved label.

## Consequences

- **Robustness is why archive is inverted.** Every hook we install is
  best-effort, because none may fail message reception — the Phase 1 and 2
  hooks are already written that way. If archive were the absence of an Inbox
  label, a hook that failed to apply Inbox would make a message vanish from the
  inbox *and* the archive. As the presence of a label, a failed hook leaves the
  message in the inbox: visible and recoverable. The cheaper storage is a
  secondary benefit, not the reason.
- A UI may still present Inbox as though it were a label. Sent, Drafts and
  Trash should be presented the same way but must be *derived* from
  `MessageState` and `chat_id`, which core already owns. Storing them as labels
  would create a second source of truth that can drift.
- The reserved label cannot be renamed or deleted. Deleting any label removes it
  from its messages and never touches the messages themselves.
- **Search stays a separate function** so upstream's version keeps its tests and
  its performance tuning, and we take no merge conflict on a function upstream
  revisits. The cost is that a caller must know to use ours.
- **`LIKE` rather than FTS** because an FTS index over decrypted mail is a
  second plaintext copy of the mailbox on disk. That matters more here than
  search latency, given the local store is the only durable copy
  ([0004](0004-local-store-and-raw-mime.md)) and at-rest encryption is still a
  later feature. `LIKE` is also what upstream's benchmarks and its 1000-row cap
  are tuned against. If search becomes too slow to use, revisit it deliberately,
  with the plaintext-at-rest question answered first.
- **Label changes arriving before their messages are parked, not dropped.**
  Core's own sync handlers warn and drop when a `Message-ID` is unknown, which
  is tolerable for a deletion — the message may never arrive — and not for a
  label, where the user would watch a label they applied on their phone
  silently fail to appear on their laptop. Parked changes live in
  `pending_msg_labels`, keyed by `Message-ID`, latest timestamp wins, and are
  drained by the existing receive hook. They expire after 90 days so changes for
  messages that never arrive do not accumulate.
- Sync is by label **name**, since row ids are assigned per device. Creation is
  therefore idempotent and case-insensitive: two devices creating "Work"
  independently converge on one label. A sync item naming a label that does not
  exist here creates it rather than failing, so a lost creation cannot leave the
  devices permanently divergent.
- One `SyncData` variant keeps the upstream patch to a single enum arm and a
  single match arm. Older builds that do not know the variant fall into
  `SyncDataOrUnknown::Unknown` and log it, which is upstream's existing
  forward-compatibility behaviour.
