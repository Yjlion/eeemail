# 0011 — Read receipts get a verified-only middle setting; ephemeral ships off

**Status:** Accepted — 2026-08-31

## Context

The plan says ephemeral messages and read receipts are both "on by default,
disableable, with per-contact overrides", and adds: *"if a contact is verified,
and in the address book, they get read receipts."*

Implementing that surfaced two things.

**Read receipts have no per-contact notion in core.** `Context::should_send_mdns`
reads one global flag and takes no correspondent. `MdnsEnabled` already defaults
to on, so "on by default" was already true; what was missing was *who*.

**Ephemeral is not the same kind of setting.** Ephemeral deletion removes the
message **locally as well as remotely**. For Delta Chat that costs you a chat
message. For eeemail the local store is the *only durable copy of the mailbox*
([0004](0004-local-store-and-raw-mime.md)), so a non-zero default means the
user's mail silently deletes itself. The plan does not name a duration, and
there is no obviously correct one.

## Decision

**Read receipts get a three-state policy**: never, verified-and-known contacts
only, or always. Always is the default. `MdnsEnabled` stays authoritative for
on-vs-off, the same relationship `EncryptionMode` has with `ForceEncryption`
([0006](0006-encryption-policy.md)).

**Ephemeral machinery is complete and ships disabled.**
`Config::EphemeralDefaultSeconds` defaults to `0`. Everything honours whatever
it is set to: a global default, per-contact overrides, and automatic
application to a conversation.

## Consequences

- The verified-only setting is what makes *"if a contact is verified, and in the
  address book, they get read receipts"* expressible as a policy rather than a
  quirk. Verified means SecureJoin, which survives an active attacker;
  in the address book (`Origin::is_known`) means you chose to know them rather
  than they mailed you once. Opportunistically learned keys do **not** count.
- **A global off is a hard off.** It beats the policy and every per-contact
  override. A user who turns read receipts off must not discover they are still
  going to some contacts; the reverse ordering would be a privacy regression,
  not a convenience.
- Read-receipt *requesting* stays global. Sending is the direction that
  discloses something about the user, so that is where per-contact control
  belongs.
- **Ephemeral shipping off is a deliberate departure from "on by default", and
  should be reviewed.** Turning it on is one config value, and every test here
  covers the on case. It ships off because the destructive reading conflicts
  directly with [0004](0004-local-store-and-raw-mime.md)'s guarantee that
  decrypted content is canonical and permanent, and because no duration was
  specified. This is a decision the user should make with the consequence in
  front of them, not one to infer.
- Per-contact ephemeral overrides compose to the **shortest**, mirroring how
  encryption overrides compose to the strictest: an override is a statement
  about that correspondent, so a group including them cannot loosen it.
- The default is applied when the **first** message is sent to a conversation,
  not on every send — otherwise turning the timer off would not stick — and not
  at chat creation, which would need hooks in every path that makes a chat. A
  bad or out-of-range value means "no timer", never an arbitrarily short one.
