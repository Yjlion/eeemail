# 0010 — Server retention is applied on arrival and is never retroactive

**Status:** Accepted — 2026-08-31

## Context

The server is a transport spool and the local database is the mailbox
([0003](0003-imap-as-transport.md), [0004](0004-local-store-and-raw-mime.md)),
so a downloaded message does not need to stay on the server. The plan settled
the policy — delete-after-download by default, keep N days, or never delete —
but not *when* it is evaluated.

That timing question is the whole risk. A user's first run of eeemail is
typically against a mailbox that already holds years of mail. A retention policy
evaluated over the whole mailbox would delete all of it from the server on first
sync, before the user has any reason to believe the local copy is trustworthy.
The action is irreversible and the user did not ask for it.

Chatmail relays make this safe by expiring server-side after 20 days regardless;
we cannot assume that, because eeemail is meant to work against the user's
existing provider.

## Decision

Delete-after-download is the default, as planned.

**The policy is evaluated on the receive path, per message, as it arrives.** It
is never applied retroactively to messages already on the server.

Deletion reuses core's existing mechanism: setting `imap.target` to the empty
string is what makes the IMAP loop mark a message `\Deleted`. We only decide
*when*, never how.

## Consequences

- Pointing eeemail at an existing mailbox cannot destroy mail that was already
  there, whatever the setting says. Only messages eeemail itself has received
  and stored are ever deleted. This mirrors the same choice made for raw-MIME
  retention in [0004](0004-local-store-and-raw-mime.md), for the same reason:
  changing a retention setting must not reach backwards and destroy data.
- A user who *wants* their existing server mail cleared needs an explicit,
  separate action. That is the right shape for it — it is a destructive bulk
  operation and should look like one.
- `keep N days` needs deferred deletion, so `server_retention` records a
  `delete_at` per `Message-ID` and housekeeping acts on it. Keyed by
  `Message-ID` because that is what the `imap` table is keyed by.
- `never delete` writes nothing at all — not even a row — so coexistence mode
  leaves the server mailbox byte-for-byte untouched, which is exactly what the
  data-safety test in `DESIGN.md` asserts.
- Deletion is scheduled only after the message is stored locally, because the
  hook sits at the same success exit as the rest of our receive-side work. A
  message that fails to be received is never deleted from the server, so it can
  be retried.
