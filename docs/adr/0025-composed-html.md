# 0025 — Composed HTML is a whitelisted alternative, never a replacement

**Status:** Accepted — 2026-09-06

## Context

The composer sent plain text only. That was never an engine limitation:
`Message::set_html` sets `Param::SendHtml`, `MimeFactory` emits the `text/html`
part from it, and upstream's own `MessageData.html` already uses that path.
eeemail's `email::compose::send` simply never called it, and `send_email` had no
parameter to carry it.

So the question was not whether we *can* send HTML, but what a client that
renders received HTML in a sandboxed frame with `default-src 'none'`
([0013](0013-desktop-ui.md)) should be willing to *produce*.

What the field does, since it constrains the answer more than our own taste
does:

- **Thunderbird** — a per-account "Compose in HTML" mode with a rich-text
  toolbar, and Shift+Write to force plain text. Editing HTML *source* is an
  add-on, never the built-in composer.
- **Proton Mail** — a rich-text toolbar, HTML underneath, and a plain-text
  toggle.
- **Delta Chat**, the fork's upstream — converged on HTML restricted to a
  whitelist (`b i u s`, `h1`–`h3`, `ul/li`, `blockquote`, `code`, `pre`), and
  explicitly rejected full HTML, Markdown-as-transport, and manual HTML entry
  by the user, which they judged poor UX. Their stated order is: make rendering
  consistent across clients first, ship an editor second.

Everyone ships a toolbar. Nobody ships a source box. Everyone sends a
`text/plain` alternative alongside.

There is a second constraint that is ours rather than the field's. A
`contenteditable` body is edited by the browser, and what the browser leaves in
the DOM is not something we chose: `document.execCommand` is deprecated, its
output differs between engines, and a paste drops in whatever the source page
had. None of that is fit to put on the wire.

## Decision

**The composer has a formatting mode, and nothing the editor produced is sent.**

1. **A toolbar over `contenteditable`**, not an HTML source box. Bold, italic,
   underline, strikethrough, heading, quote, bulleted and numbered lists, code,
   link.

2. **The body is re-emitted, not extracted.** `desktop/src/richtext.ts` walks
   the DOM itself and writes out a fixed set of tags — Delta Chat's list, plus
   `p`, `br` and `a` — and unwraps everything else, keeping its text. An
   element we will not carry loses its markup, never its content. A link whose
   scheme is not `http`, `https` or `mailto` becomes its own text, because a
   `javascript:` href in an outgoing message is an attack on whoever opens it
   and this is the last point at which we are the ones deciding. The scheme
   itself is supplied where the user types the address, not where it is
   serialised: `example.com` becomes `https://example.com` and a bare address
   becomes a `mailto:`, so `safeHref` stays a whitelist with no guessing in it
   and parses with **no base URL**. Resolving a scheme-less href against a
   placeholder would put a link to a domain we invented into someone's mail,
   looking exactly like one the user chose.

3. **`text/plain` is always sent, and is derived from the same walk.** `html` is
   an alternative *beside* `text`, never instead of it. A correspondent whose
   client shows plain text reads the message rather than a blank body. The two
   parts come from one traversal so they cannot drift apart.

4. **An unformatted message goes out unformatted.** If the walk produced nothing
   a plain-text reader would miss — only `p` and `br` — `html` is `null` and the
   message is an ordinary plain-text mail. Formatting mode does not make every
   message HTML.

`email::compose::send` gains a trailing `html: Option<&str>`; `send_email` gains
a sixth positional parameter.

## Consequences

- eeemail now sends less HTML than it is willing to receive. That asymmetry is
  deliberate: the reading pane frames message HTML because HTML in mail is not a
  safe format, and being more permissive as a sender than as a reader would be
  the wrong way round.
- The whitelist is upstream's, so a Delta Chat client renders what we send
  without either side special-casing the other.
- `send_email` changed arity, and **yerpc checks positional arity exactly** — a
  missing trailing `Option` is `invalid params`, not `None`. Every caller had to
  move in the same commit, including the three live-pass scripts that no CI job
  runs. See `docs/handoff.md`.
- No dependency was added. A rich-text library in the process that renders
  untrusted mail is the thing [0013](0013-desktop-ui.md) declined a framework
  over.
- `execCommand` is deprecated and will eventually stop working. When it does,
  only the toolbar's *commands* break; the serialiser is independent of how the
  DOM came to be, so replacing the editing layer does not touch what goes on
  the wire.

## Alternatives considered

**An HTML source box with a live preview.** Simplest to build, no
`contenteditable` quirks, and the preview could reuse the sandboxed frame the
reading pane already has. Rejected because it is the one approach none of the
researched clients offers as its primary composer, and because it hands the
user a way to emit tags the reading pane was not built to show.

**Markdown.** Pleasant to type and popular in chat clients. Rejected for the
reason Delta Chat rejected it: parsing is inconsistent between implementations,
so what the sender saw and what the recipient's client renders are only loosely
related — and the alternative is a Markdown dependency in the renderer.

**Sending HTML only, with no plain-text part.** What a lot of commercial mail
does. Rejected outright: it turns a formatting choice into a delivery failure
for anyone whose client shows `text/plain`.

## Amendment — 2026-09-15

**The editing layer and the whitelist have changed. Everything else here stands.**

The formatted mode is now a [Squire](https://github.com/fastmail/Squire) editor
rather than `contenteditable` with `execCommand`, so "no dependency was added"
no longer holds. The whitelist gains `<span>` and a `style` attribute rebuilt
from a few checked properties (colour, highlight, generic font family, keyword
font size, alignment), and the same filter now also parses everything Squire
loads or has pasted into it. The `text/plain` part, the unformatted-mail rule and
the link rules are unchanged. See [0030](0030-the-composer-edits-with-squire.md).
