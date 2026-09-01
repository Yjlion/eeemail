# 0005 — Organize with labels/tags + archive, not a folder tree

**Status:** Accepted — 2026-08-31 · One consequence amended by [0009](0009-labels-and-search.md)

## Context

With no IMAP folders ([0003](0003-imap-as-transport.md)), all organization is
client-side, so we are free to choose the model rather than inherit it. The two
candidates are a familiar folder tree (Inbox, Archive, Sent, user folders) and
Gmail-style labels with an archive action.

## Decision

Labels/tags plus archive. System labels (Inbox, Archive, Sent, Drafts, Trash)
alongside user-defined ones. Archive is simply the removal of the Inbox label.

## Consequences

- `msg_labels` is many-to-many by construction, so a message can carry several
  labels without the duplication or aliasing hacks a tree needs.
- Labels fit a threaded conversation view better: a thread whose messages carry
  different labels is representable, where "which folder is this thread in?" has
  no good answer.
- Labels are local-only and synced between the user's own devices over core's
  existing sync-message channel. They are never expressed to the server.
- Users who want folders can approximate them with mutually-exclusive labels,
  but we do not enforce exclusivity. If that proves to be a real gap, a
  folder-like UI over labels can be added without a schema change.
