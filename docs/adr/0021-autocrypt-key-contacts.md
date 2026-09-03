# 0021 — An Autocrypt header makes a key-contact, and says so

**Status:** Accepted — 2026-09-02

## Context

[ADR 0006](0006-encryption-policy.md) makes opportunistic encryption the
default: "encrypt whenever a key is known, cleartext otherwise". The first live
end-to-end pass against a real server showed that eeemail never encrypts
anything under that rule, and cannot.

Upstream `v2.59` does not decide encryption by key availability. It decides by
*contact type*. A contact row carries a `fingerprint` or it does not; a `Single`
chat is encrypted only when its contact carries one (`Chat::is_encrypted`,
`core/src/chat.rs:1690`), and a contact acquires a fingerprint from exactly two
places:

- **A signed message.** `receive_imf.rs:588` reads the fingerprint from
  `mime_parser.signature` — from the OpenPGP signature, not from the Autocrypt
  header.
- **SecureJoin.** A QR code scanned in person.

Autocrypt peerstates are gone from core entirely (`grep -c peerstate
core/src/receive_imf.rs` → 0). A cleartext message carrying an `Autocrypt:`
header still has its key imported into `public_keys`
(`core/src/mimeparser.rs:446`), but no contact is ever attached to it.

So the bootstrap never happens. Alice mails Bob in cleartext, advertising her
key; Bob's client stores the key and creates an address-contact; Bob replies,
and because his contact for Alice carries no fingerprint, the reply is cleartext
too — advertising his key, to the same end. Neither can ever send the first
encrypted message, so neither ever sends a signed one, so neither ever becomes a
key-contact. Two eeemail users who do not meet in person to scan a QR code
exchange plaintext forever, and ADR 0006's default is unreachable.

This is a deliberate upstream change, and the reasoning behind it is sound: an
Autocrypt header is unauthenticated. Anyone who can write the `From` line can
write the `Autocrypt` line. Trusting it means trusting the network, which is the
thing SecureJoin exists not to do.

But upstream is a chat client whose users are expected to scan each other's
codes. eeemail is an email client. Its users have correspondents they will never
meet, and the choice for those correspondents is not "authenticated encryption
or opportunistic encryption" — it is "opportunistic encryption or cleartext".

## Decision

**An `Autocrypt:` header on an incoming message creates a key-contact for the
`From` address**, using the fingerprint core has already imported.

**It creates an ordinary key-contact, never a verified one.** Verification stays
what it was: SecureJoin, in person, resistant to an active attacker. This ADR
adds a rung below that rung, and does not touch it.

**The distinction is shown, not buried.** Encryption to an Autocrypt-learned key
is marked as encrypted and *not* marked verified — which is what the reading
pane already does, because [ADR 0006](0006-encryption-policy.md) and
[ADR 0013](0013-desktop-ui.md) made "encrypted" and "verified" separate badges
from the start. A user who wants the stronger claim has a QR code to scan, and
the UI keeps telling them so.

**Adopted only when the message is not signed.** A signed message already gives
core a fingerprint it can check against the signature, which is strictly better
evidence. This only fills the gap where there is none.

**It lives in `core/src/email/`, and hooks the receive path where eeemail
already hooks it.** No new patch site: the best-effort block at the end of
`receive_imf_inner` is where every other eeemail receive hook lives, and like
them, failing to derive a key-contact must never fail reception.

## Consequences

- **ADR 0006's default becomes reachable.** "Encrypt whenever a key is known" is
  true again, for the first time since the fork moved to `v2.59`.
- **We accept unauthenticated keys.** An active attacker who can rewrite mail in
  flight can substitute their own `Autocrypt` header on first contact and read
  everything after. This is precisely Autocrypt's documented threat model, and
  precisely what upstream removed. What it buys is protection against the
  passive adversary — the one who reads stored mail at a provider — which is the
  adversary most people actually have.
- **It is strictly better than the status quo, which is cleartext.** The
  alternative on offer is not a verified key. It is no encryption at all, to an
  adversary who does not need to be active to read it.
- **Key changes are core's problem, not ours.** We create the contact and let
  core's existing handling of a changed fingerprint apply unchanged, rather than
  growing a second, subtly different key-change policy.
- **The message that carried the header stays in its original chat.** It arrived
  in cleartext and is displayed as such; only subsequent outgoing mail benefits.
  Re-homing a delivered message would rewrite history to make a claim about the
  past that is not true.
- **Verified-only affordances are unaffected.** Read receipts
  ([ADR 0011](0011-receipts-and-ephemeral.md)) and any structured-email
  affordances ([ADR 0016](0016-structured-email.md)) that require verification
  still require it. This ADR moves nobody into the verified set.

## Amendment — 2026-09-02, after the interop pass

`scripts/interop-pass.py` confirmed every step of the reasoning above against
upstream's own released binary: a stock client's reply to our cleartext really
is cleartext, we really do adopt the key from its header, and it really does
decrypt and verify what we then send. The bootstrap works, and against a second
implementation rather than against ourselves.

It also found the limit. Upstream defaults `force_encryption` on, which is not
advisory — a stock client refuses to send unencrypted mail (`chat.rs:2958`),
refuses to *download* it (`imap.rs:1694`), and trashes it if it arrives anyway
(`receive_imf.rs:509`). So the first consequence above, "ADR 0006's default
becomes reachable", holds for correspondents on ordinary mail, and **not** for a
Delta Chat client in its shipped configuration: our opening cleartext message is
discarded before it is parsed, so no Autocrypt header is ever exchanged and
there is nothing to adopt. With such a correspondent the only paths to
encryption are SecureJoin, or their turning that setting off.

This does not change the decision. It narrows the claim: the rung this ADR adds
sits below verification and above cleartext, and it is reachable with anyone
whose client will accept a cleartext message at all.

## Sources

- [Autocrypt Level 1](https://autocrypt.org/level1.html) — header format, `prefer-encrypt`, and the explicit statement that Autocrypt protects against passive, not active, adversaries
- `core/src/chat.rs:1690` — `Chat::is_encrypted`, the rule this works within
- `core/src/receive_imf.rs:588` — where a fingerprint comes from today
- `core/src/mimeparser.rs:444` — the Autocrypt key core already imports
