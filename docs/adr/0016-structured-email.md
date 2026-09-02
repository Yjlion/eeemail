# 0016 — Structured email is parsed for everyone and acted on only for trusted senders

**Status:** Accepted — 2026-09-01

## Context

[Structured email](https://structured.email/) is a machine-readable
representation of a message carried alongside the human-readable one, so a
client can show a parcel as a delivery, a booking as an itinerary, and an
out-of-office as an absence with a date and a deputy. The IETF has a working
group on it ([SML](https://datatracker.ietf.org/wg/sml/about/)), and the parts
we care about are settled enough to build against:

- Structured data is carried in an **`application/ld+json`** body part.
- That part carries **`Content-Purpose: Machine-readable`**, which is what stops
  it appearing to the user as a mystery attachment.
- Three arrangements: `multipart/alternative` when the structured data is a
  *full* representation of the human-readable content, `multipart/related` when
  it is *partial*, and `multipart/mixed` when it is neither.
- Two IMAP keywords are registered, `$hasStructuredData` and `$MRM`.
- Deployed senders today mostly do not do any of that. They emit
  Schema.org-for-Email: a `<script type="application/ld+json">` block inside the
  HTML body, which is what Gmail reads.

The awkward part is not parsing. It is that structured data exists to drive
**affordances** — a "track this parcel" button, a calendar entry, a rescheduled
send. An affordance rendered from attacker-controlled data is a phishing
primitive with a nicer typeface than the attacker could manage alone, and
`draft-happel-structured-email-trust` exists because the working group knows it.

eeemail already answers "how much do I trust this sender?" twice: encryption
and verification state per message ([0006](0006-encryption-policy.md)), and the
gating verdict on arrival ([0018](0018-contact-gating.md)).

## Decision

**Adopt SML's mechanism.** Parse `application/ld+json` parts marked
`Content-Purpose: Machine-readable` in all three multipart arrangements, and
fall back to `<script type="application/ld+json">` in the HTML body so that mail
from senders who ship today's de-facto format is not ignored.

**Store the verdict with the data.** Each extracted object is stored with a
`trusted` flag computed once, at receive, from the message's encryption and
verification state and its gating verdict.

**Trusted data may be acted on. Untrusted data is shown inert** — rendered as
labelled fields, with no links, no buttons and nothing that initiates a request.

**Ignore the IMAP keywords.** `$hasStructuredData` and `$MRM` are server-side
state, and eeemail holds none ([0003](0003-imap-as-transport.md)).

## Consequences

- The trust rule is the *same* rule as everywhere else in the client, not a
  second one. A sender who cannot get their mail into the inbox
  ([0018](0018-contact-gating.md)) cannot get a button in front of the user
  either, and that follows without a line of structured-email-specific policy.
- **Parsing is unconditional; acting is not.** Refusing to parse untrusted data
  would mean the user cannot see what a message claims about itself, which is
  information they may want precisely because they distrust the sender.
- Rendering untrusted structured data at all is a deliberate risk we accept: an
  attacker gets to put well-formatted text on screen. They can already do that
  with HTML. What they do not get is a control that does something.
- Unknown Schema.org types are shown as generic labelled data rather than
  dropped. The vocabulary is large and grows; a client that renders only what it
  recognises silently discards the rest of the message's meaning.
- Malformed JSON-LD is dropped with a log line and never fails reception, the
  same rule raw MIME retention follows ([0004](0004-local-store-and-raw-mime.md)):
  an enhancement must not be able to lose a message.
- We do not *send* structured data yet. Sending is a separate decision — it
  means committing to a vocabulary and to what our own mail asserts — and
  nothing in this ADR depends on it.

## Sources

- [SML Core](https://datatracker.ietf.org/doc/draft-ietf-sml-structured-email/) — media types, `Content-Purpose`, multipart arrangements, IMAP keywords
- [SML Trust and Security](https://datatracker.ietf.org/doc/draft-happel-structured-email-trust/)
- [SML Use Cases](https://datatracker.ietf.org/doc/draft-ietf-sml-structured-email-use-cases/)
- [Schema.org for Email](https://structured.email/related_work/frameworks/schema_org_for_email.html) — what deployed senders emit today
