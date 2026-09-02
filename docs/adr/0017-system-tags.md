# 0017 — System tags: derived where possible, stored where they must be

**Status:** Accepted — 2026-09-01 · Supersedes the "archive is the removal of the Inbox label" wording in [0005](0005-labels-not-folders.md)

## Context

[0005](0005-labels-not-folders.md) chose tags over a folder tree and said
"System labels (Inbox, Archive, Sent, Drafts, Trash) alongside user-defined
ones. Archive is simply the removal of the Inbox label."

Two things happened since.

[0009](0009-labels-and-search.md) **inverted archive**: it is the *presence* of a
reserved `Archive` label, because every hook we install is best-effort and an
absence-based inbox would make a message vanish from both views when a hook
failed. Only `Archive` was built.

Phase 3 then **deferred Sent, Drafts and Trash** with the note that they "must
be derived from `MessageState` and `chat_id`, which core already owns. Storing
them would create a second source of truth." That was right, and it left the
client with exactly one system tag and a sidebar that could show Inbox and
Archive and nothing else.

Meanwhile the product premise is that **the mailbox organizes itself**. Most
people do not want to file mail, and a client whose only organizational
primitive is a tag the user must create by hand has handed them a filing job
with extra steps.

Two more system tags also arrived with real requirements: `Holding`
([0018](0018-contact-gating.md)) and `Trash` as the destination for expired
ephemeral messages ([0019](0019-recoverable-ephemeral-expiry.md)). Both carry a
**purge deadline**. A derived tag cannot carry a deadline, because there is no
row to put it on.

## Decision

Three kinds of tag, distinguished by whether they need a row.

**Derived** — `Inbox`, `Sent`, `Drafts`. Computed from state core already owns
(`MessageState`, `chat_id`, direction) plus the absence of any stored system
tag. No rows, so no second source of truth and nothing to keep consistent.
`Inbox` is *incoming, and not archived, held or trashed*.

**Stored and reserved** — `Archive`, `Trash`, `Holding`. Rows in `labels`,
because each is either a user action that must survive a failed hook (`Archive`,
per 0009) or carries a purge deadline (`Trash`, `Holding`). Reserved names
cannot be renamed or deleted, which `labels` already enforces.

**User tags** — everything else, unchanged.

One resolver returns a message's complete tag set, derived and stored together,
so a caller asks once.

## Consequences

- **The user files nothing and still gets a working mailbox.** Inbox, Sent,
  Drafts, Archive, Trash and Holding exist on a fresh account with no setup.
  Tags are additive on top of that, not a prerequisite for it.
- The derived/stored split is a rule, not a list: a system tag gets a row **only**
  if it carries state core does not already have. That is what stops the set
  drifting into a second mailbox model shadowing core's.
- A message can be in `Archive` and a user tag and a thread whose other messages
  are in the Inbox. That is the property [0005](0005-labels-not-folders.md)
  chose tags for, and it survives here.
- `Sent` and `Drafts` cost nothing to add and cannot desynchronise, because
  there is nothing to synchronise. They also cannot be applied by hand — you
  cannot tag a received message `Sent` — which is correct and will read as a
  limitation to anyone expecting Gmail's labels.
- Stored system tags sync between the user's devices by name over the existing
  channel, like every other tag. Their purge deadlines do **not**: a deadline is
  a local scheduling detail, and syncing it would let a stale device resurrect
  or prematurely destroy mail on a fresh one.
- The reserved names are not localised. A tag name is an identifier on the sync
  wire ([0009](0009-labels-and-search.md)); translating it would make two
  devices in two locales disagree about which tag is which. The UI translates
  the display string.
