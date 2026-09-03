# `server/` — mail server for eeemail

eeemail treats IMAP/SMTP as **transport, not storage**
([ADR 0003](../docs/adr/0003-imap-as-transport.md)): mail is downloaded,
decrypted, stored locally and removed from the server. The server is a spool.

This directory holds a mail server configured for that role, adapted from
[`chatmail/relay`](https://github.com/chatmail/relay) (MIT) with its **relay
account provisioning removed** — accounts here are traditional `user@domain`,
not create-on-login random addresses
([ADR 0007](../docs/adr/0007-server-template.md)).

**It is a convenience, not a requirement.** eeemail must work against any
standards-compliant IMAP/SMTP provider, including a user's existing mailbox.
Integration tests run against a mainstream provider too, so that we never
quietly depend on our own server's quirks.

## `compose/` — test server

Postfix + Dovecot in one container. Used by CI and local development.

```sh
cd server/compose
docker compose up -d --build
python3 smoke-test.py

STRICT_E2EE=1 docker compose up -d --build --force-recreate
python3 smoke-test.py --strict
```

Without the compose plugin, plain Docker works too:

```sh
docker build -t eeemail/test-mail:latest server/compose
docker run -d --name eeemail-mail --hostname eeemail.test \
  -p 2525:25 -p 2587:587 -p 2465:465 -p 2143:143 -p 2993:993 \
  eeemail/test-mail:latest
python3 server/compose/smoke-test.py
```

### Two profiles, because we have three encryption modes

[ADR 0006](../docs/adr/0006-encryption-policy.md) gives eeemail strict,
opportunistic and lenient modes. They need different servers to test against:

| Profile | Models | Exercises |
|---|---|---|
| **permissive** (default) | An ordinary email provider | Opportunistic and lenient modes; cleartext interop with normal mail clients |
| **strict** (`STRICT_E2EE=1`) | A chatmail-style relay | Strict mode; submission-time metadata minimization |

The difference is `submission_header_cleanup`, which strips `Received`,
`X-Originating-IP`, `X-Mailer` and `User-Agent`, and replaces `Subject` with
`[...]`.

**That Subject rewrite is why the profiles must stay separate.** Under strict
mode all mail is encrypted, the real subject lives in the protected header
inside the encrypted part, and the outer one is already a placeholder — so
minimizing it is free. On a permissive server eeemail sends cleartext, and the
same rule would silently destroy real user data.

### Defaults

| | |
|---|---|
| Domain | `eeemail.test` (`MAIL_DOMAIN`) |
| Accounts | `alice bob carol dana erin frank grace heidi ivan`, password `<name>pw` (`ACCOUNTS`, `user:password` pairs) |
| Ports | 2525→25, 2587→587, 2465→465, 2143→143, 2993→993 |

`docker-compose.yml` is the source of truth for the account set. The bare
`docker run` recipe above passes no `-e ACCOUNTS`, so it gets `entrypoint.sh`'s
fallback of `alice` and `bob` only — as does the `mail-server` job in CI. Any
script needing more than those two must be run against the compose file.

`carol` exists so that `scripts/e2e-pass.py` can send a message with a `Cc:` to a
third real mailbox and check the header survives the round trip. Two accounts
cannot tell a dropped `Cc` from a delivered one.

`dana`, `erin`, `frank` and `grace` are two independent pairs for
`scripts/interop-pass.py`, which runs eeemail's engine against upstream's. They
do not reuse `alice`/`bob` because those carry state from every `e2e-pass.py`
run — including a completed SecureJoin, which would make the Autocrypt
bootstrap the interop pass exists to check unobservable. Two pairs rather than
one because SecureJoin is tested in both directions, and a second direction
between an already-verified pair asserts nothing. **Within each pair, the
earlier name is eeemail's and the later is upstream's**: `dana` and `frank` run
our fork, `erin` and `grace` run the stock released binary.

`heidi` and `ivan` are `scripts/gpg-interop-pass.py`'s, which runs eeemail
against a GnuPG client rather than against another Delta Chat engine. `heidi` is
GnuPG and never runs any of our code; `ivan` is eeemail. They are a pair of
their own for the same reason the interop pass has its own: a mailbox carrying
another pass's completed handshake would make the key exchange this one watches
unobservable.

### What the smoke test covers

Delivery and authentication, plus the properties whose absence would be a
security bug: sender-login enforcement, no open relay on port 25, inbound mail
to local accounts still accepted, and plaintext IMAP auth refused without TLS.
It also asserts `IDLE`, `METADATA` and `QUOTA` are advertised — `METADATA`
matters because core's `calls.rs` reads STUN/TURN ICE servers from it.

### Deliberately not production

Self-signed certificate regenerated on start, plaintext passwords in a
`passwd-file`, no DKIM/SPF/DMARC, no MTA-STS, `ssl = yes` rather than
`required`. DKIM and MTA-STS need real DNS and belong in `deploy/`.

Because the certificate is self-signed, clients must be told to accept it —
core exposes `imap_certificate_checks=accept_invalid_certificates`.

## `deploy/` — production deployment

**Deliberately not built** (decided 2026-09-01). `compose/` covers what the
project needs today: a disposable target to develop and test application
functionality against. Everything `deploy/` would add — OpenDKIM, `unbound`,
`acmetool` for real certificates, MTA-STS, DNS record generation, `filtermail`
at the perimeter, and `chatmail-expire`-style retention — depends on a real
domain and real DNS. None of it can be written honestly, let alone verified,
without one, and configuration nobody has ever run is worse than no
configuration: it looks like a deployment path and is not.

This is not a gap in eeemail. The client must work against any
standards-compliant IMAP/SMTP provider, and a self-hosted server is a
convenience for people who want one. When there is a domain to deploy to, the
starting point is [`chatmail/relay`](https://github.com/chatmail/relay)'s
`cmdeploy` tree, adapted per [ADR 0007](../docs/adr/0007-server-template.md).

One thing to carry across when it happens: relay's
`delete_inactive_users_after` (90 days) deletes **entire mailboxes**. That is
reasonable for disposable relay identities and unacceptable for a person's real
address; it stays off.
