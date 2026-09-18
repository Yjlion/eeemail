# 0032 — The composer decides the signature and the padlock, per message

**Status:** Accepted — 2026-09-18 · amends [0006](0006-encryption-policy.md) and
the signature behaviour recorded with [0025](0025-composed-html.md)

## Context

The composer was redone to look like other mail clients: an icon toolbar, a
colour picker, Cc and Bcc added only when wanted, attachment and importance as
toolbar icons, a signature button, spell checking, and a toggle for whether a
message is encrypted. Most of that is presentation. Two parts are not:

- **The signature.** The engine appended the configured signature to every
  message at render time, after a `-- ` line, where the user never saw it.
  Other clients put it in the body, where it can be read, edited for one
  message, or removed. A button that inserted it into the body while the engine
  went on appending it would sign every message twice.
- **The padlock.** Whether a message is encrypted was decided entirely by
  policy ([0006](0006-encryption-policy.md)): the global mode, per-contact
  overrides, and whether a key is held. The composer could only report the
  outcome. The user asked to choose it per message.

## Decision

**`send_email` takes an eighth parameter, `options`:
`{ encryption: "auto" | "required" | "plaintext", signatureInBody: bool }`.**
`null` is `auto` and `false`, which is exactly what every message did before.
The choices are stored in a side table, `msg_send_options` (migration 175),
keyed by `msgs.id` and absent when default, the arrangement
[0029](0029-importance-travels-on-the-wire.md) uses for importance.

**Not in `msgs.param`**, although `ForcePlaintext` and `GuaranteeE2ee` are
params and say exactly this. `chat::send_msg` strips both from any message that
is not brand new, and `compose::send` saves every message as a draft first —
that is what gives it an id for the recipient set. A param set at compose time
is erased before anything reads it. `policy::prepare_send`, already hooked in
`chat.rs`, reads the stored choice and sets the param at the point core reads
it. No new patch site.

### The padlock

- **`required`**: end-to-end or not at all. Refused before anything is stored
  if any To, Cc or Bcc recipient has no key, naming them — rather than, under
  the opportunistic default, encrypting to some and recording the rest as
  undelivered.
- **`plaintext`**: cleartext, even to recipients whose key is held. **Refused
  wherever the policy says end-to-end only**, globally or by an override on any
  recipient. The padlock cannot outvote a setting the user made about a
  correspondent; this amends 0006 only by adding a way to be *less* strict
  where 0006 already allowed cleartext.
- **`auto`**: 0006 unchanged. The composer never sends it — its padlock is
  always on or off — but every other caller gets it by default.

The composer asks `get_send_readiness` while the user types. It reports the
effective mode, who has no key, and whether the padlock is locked. It is
read-only and creates no contacts, because it runs on every keystroke in an
address field. The padlock starts closed when everyone has a key and open
otherwise, is locked closed under end-to-end only, and Send is disabled while
it is closed and someone has no key. The engine repeats every check on send, so
a composer that got this wrong would be refused rather than obeyed.

### The signature

**The composer places the signature in the body, after a `-- ` line, above any
quote, when a new message opens.** A button removes it or puts it back. With
`signatureInBody`, the configured signature is not appended. Whatever follows
the body's last `-- ` line is the signature, and a body with none is unsigned,
because the user removed it.

**The signature is split off the body before sending, not left in it.**
Upstream escapes every `-- ` line in message text to `-\u{200B}- `
(`simplify::escape_message_footer_marks`), so that Delta Chat, which drops
everything after a footer mark, does not lose text the user wrote. For the one
line that *is* the separator, that escape is wrong: the signature reached every
recipient under a separator with a zero-width space in it, which no client
recognises. This was found by the test, not by review. So `compose::send`
splits the plain text at its last separator line. It stores the part below as
this message's signature, and `signature::load_for` hands it to the renderer as
the footer, which goes out after a real `-- `. The HTML part is not escaped and
already carries the signature the user saw, so it is left alone: that
per-message signature has empty HTML, and `append_to_html` appends nothing.

## Consequences

**`send_email`'s arity moved again, and every caller moved with it** — the
composer and all four live-pass call sites in `scripts/e2e-pass.py`,
`scripts/interop-pass.py` and `scripts/gpg-interop-pass.py` (eleven calls). As
[0029](0029-importance-travels-on-the-wire.md) recorded, `yerpc` rejects a
short call as `invalid params`, and no CI job runs those scripts.

**Everything after the last `-- ` is treated as the signature**, including a
reply's quote when the user signed above it. The text reaches the wire in the
order it was written. But the quote now sits below the separator, where a
recipient's client that folds signatures will fold it too. Other clients that
sign above the quote behave the same way.

**The local copy of a sent message shows the body without the signature**, as
it did when the engine appended it. The signature is on the wire, in both
parts, and in the retained raw MIME.

**A message sent by anything other than the composer is signed as before.**
`signatureInBody` defaults to false, so the CLI, the live passes and any other
client get the configured signature appended.

**Spell checking is the webview's.** WebView2 and WKWebView honour
`spellcheck` natively. WebKitGTK needs it switched on for the web context, in
the locale's language, which the shell does at startup on Linux. There are no
dictionaries or settings of eeemail's own.
