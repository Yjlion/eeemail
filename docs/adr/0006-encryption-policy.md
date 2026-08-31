# 0006 — Opportunistic encryption by default, with strict and lenient modes

**Status:** Accepted — 2026-08-31

## Context

eeemail is an end-to-end-encryption-first client, but it is also an email
client. Those pull against each other when the recipient has no known key.

The endpoints differ in their constraints, too. A chatmail-style relay running
`filtermail` rejects unencrypted mail at the perimeter, so cleartext is not even
possible. A classic email provider permits it. And Delta Chat's own history is
instructive: the original client was opportunistic, and v2 made encryption
mandatory only for chatmail profiles while classic email profiles retained the
ability to send cleartext.

## Decision

Three modes, user-selectable, with **opportunistic as the default** — the
original Delta Chat behavior:

| Mode | Behavior |
|---|---|
| **Strict** | E2E only. Refuse to send to recipients without a key; do not process incoming cleartext. Maps onto core's existing `ForceEncryption`. |
| **Opportunistic** *(default)* | Encrypt whenever a key is known, cleartext otherwise. Unencrypted messages are visually distinct. |
| **Lenient** | Allow cleartext freely, without per-message friction. |

Autocrypt `prefer-encrypt` is set to `mutual`, so peers learn that we want
encryption and reply encrypted where they can.

Per-contact overrides sit on top of the global mode, in
`contact_policy(contact_id, ..., encryption_mode)`.

## Implementation note (found in Phase 1)

Upstream core declares `ForceEncryption` with `#[strum(props(default = "1"))]`
in `core/src/config.rs` — **strict is upstream's default**, not opportunistic.
With it on, `receive_imf_inner` discards incoming cleartext outright
(`core/src/receive_imf.rs`: "Fetched unencrypted message, ignoring") and
`create_send_msg_jobs` refuses to send to a contact with no key.

So adopting an opportunistic default is a deliberate change to that config
default, not merely a setting we expose. That is Phase 4 work, but it is worth
knowing now: until it lands, any test involving cleartext must call the
existing `TestContext::allow_unencrypted()` helper, and our three modes map
onto `ForceEncryption` as strict = on, opportunistic/lenient = off — meaning
the opportunistic/lenient distinction needs state of its own rather than
riding on this one flag.

## Consequences

- Reach is preserved: eeemail can talk to anyone with an email address, which is
  the difference between an email client and a closed messenger.
- Encryption state must be surfaced per-message in the data model and clearly in
  the UI. Users in opportunistic mode need to be able to tell, at a glance,
  which messages were protected.
- Strict mode is the server-side `filtermail` policy expressed client-side; the
  two compose, and a strict client on a filtermail relay is defence in depth.
- Lenient mode exists so that the client does not nag users whose correspondents
  simply do not use encryption. It is a deliberate choice, not a default.
