# 0007 — Ship a chatmail-derived Postfix/Dovecot template with traditional accounts

**Status:** Accepted — 2026-08-31

## Context

`chatmail/relay` is a hardened, transport-oriented mail server built from
Postfix, Dovecot, OpenDKIM and a `filtermail` policy daemon, deployed via
`cmdeploy`. Its configuration is a very good fit for our transport-only model
([0003](0003-imap-as-transport.md)) and gives us a reproducible integration-test
target.

Its *addressing* model is not a fit. Chatmail relays hand out random
nine-character addresses through `doveauth` create-on-login provisioning
(`username_min_length` and `username_max_length` both pinned to 9). eeemail is
for people's real addresses.

We want the server configuration. We do not want the relay.

## Decision

Ship `server/` — a deployment template adapted from `cmdeploy`, plus a
docker-compose variant used by CI.

**Adopt:** Postfix (strict-TLS-only, ports 25/587/465, per-user rate limiting,
size caps); Dovecot (Maildir spool, quota, connection limits, IMAP `METADATA`);
OpenDKIM (sign outbound, require valid inbound signature with `d=` matching the
`From:` domain); `filtermail` perimeter enforcement; expiry (`delete_mails_after`
20d, `delete_large_after` 7d for >200k); unbound, acmetool and DNS record
generation for MX/SPF/DKIM/DMARC.

Keep IMAP `METADATA` specifically: core's `calls.rs` reads STUN/TURN servers
from it, which is what makes Phase 8 calls and device sync work.

**Drop:** `doveauth` create-on-login and `newemail.py` random-address
generation, replaced by ordinary provisioning of `user@domain`; the 9-character
username pinning; `delete_inactive_users_after` (90d — it deletes entire
mailboxes, which is acceptable for disposable relay identities and unacceptable
for a person's real address); the iroh relay service; and push forwarding to
`notifications.delta.chat`, a third-party dependency we would want self-hosted
if we want it at all.

## Consequences

- CI gets a real, E2EE-enforcing mail server to test against, rather than mocks.
- Self-hosters get a supported path that is not a chatmail relay.
- **The template is a convenience, not a requirement.** eeemail must work
  against any standards-compliant IMAP/SMTP provider, including the user's
  existing mailbox. To keep us honest, integration tests also run against at
  least one mainstream provider, so we never quietly become dependent on our own
  server's quirks.
- We inherit maintenance of a server configuration, which is a real ongoing
  cost, and a security-sensitive one.
