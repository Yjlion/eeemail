# 0014 — A message carries its own recipients, and Cc/Bcc go through the same key path as members

**Status:** Accepted — 2026-09-01

## Context

Phases 2, 4 and 7 each deferred the same thing. In core, who receives a message
follows entirely from chat membership: `MimeFactory` derives the `To` header,
the SMTP envelope *and* the encryption key set from `chats_contacts`, and emits
no `Cc` header at all. A composer with To/Cc/Bcc fields built on that would have
fields the engine ignores, so Phase 7 shipped as a reading client.

The hard part was never the header. It is that copying someone brings a
recipient whose key state is unknown into a message that may be encrypted — and
that question already has an answer, in [0006](0006-encryption-policy.md).

## Decision

**A message may carry extra recipients of its own**, stored in `msg_recipients`
before it is sent. `MimeFactory` reads them and adds them to the header, the
envelope and the key set.

**Every Cc and Bcc address is resolved to a `ContactId`**, creating the contact
if needed.

**Bcc goes into the envelope and the key set, and into no header.**

## Consequences

- Resolving addresses to contacts is not bookkeeping. It makes "do we have a key
  for this person?" *the same question* it already is for a chat member, so
  [0006](0006-encryption-policy.md)'s policy applies unchanged rather than
  growing a second, subtly different path for Cc. Strict refuses; lenient sends
  the whole message unencrypted rather than dropping anyone; opportunistic
  encrypts to those who have keys and records the rest in `msg_undelivered`.
- Extra recipients are merged **before** the encryption branch, not inside it.
  Placing them in the encrypted branch — the first version of this — silently
  dropped every Cc from unencrypted mail.
- **`record_undelivered` had to learn about `Cc`.** It compared the `To` header
  against the envelope; a copied recipient dropped for want of a key was
  invisible to it. A recipient nobody told you about is no less missing for
  having been copied rather than addressed.
- **The send path had to stop overwriting the recipient set.** It rebuilt
  `msg_recipients` from the `To` header after sending, which would have erased
  the composer's Cc and Bcc. It now records what actually went out and carries
  Bcc across, because Bcc appears in no header and cannot be recovered from the
  rendered message — and "who did I send this to?" is a question a sent message
  has to be able to answer.
- A message that copies nobody renders byte-identically to before: the `Cc`
  header is absent, not empty.
- Someone already covered by the chat is not duplicated into `Cc` or added to
  the envelope twice, so Cc'ing a participant — a normal thing to type — is a
  no-op rather than a double delivery.
- Recipient sets attach to a **draft**, because `send_msg` preserves a draft's
  `msg_id`. That is also how a composer naturally works.
