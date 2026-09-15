# 0030 — The composer edits with Squire, and sends a little more style

**Status:** Accepted — 2026-09-15 · amends [0025](0025-composed-html.md)

## Context

[0025](0025-composed-html.md) gave the composer a formatting mode: a toolbar
over a bare `contenteditable`, driven by `document.execCommand`, with a
serialiser (`desktop/src/richtext.ts`) that re-emits the DOM as a fixed tag set
so nothing the browser produced reaches the wire. It also said *no dependency
was added*, and that `execCommand` would eventually stop working, at which point
only the toolbar's commands would need replacing.

Two things have moved since:

- **The editing layer was the weak half.** `execCommand` is deprecated, its
  output differs between engines, and eeemail ships on three of them (WebView2,
  WebKitGTK, WKWebView). Toggling a format off, keeping a caret on an empty
  line, undo that knows what a formatting change was, and a paste that arrives
  as something the user can keep editing are exactly what it does badly.
- **The user asked for more formatting**: text colour, highlight, font face,
  font size and alignment, which 0025's whitelist strips on send.

[Squire](https://github.com/fastmail/Squire) is Fastmail's editor: a
contenteditable engine built for composing email, MIT, a single ES module with
no runtime dependencies, and small enough to read. It normalises the DOM itself
rather than asking the browser to, and exposes formatting as methods instead of
command strings.

## Decision

**The composer's formatted mode is a Squire instance, and the filter that
decides what is sent is also the filter the editor parses with.**

1. **Squire `2.4.9`, pinned exact.** The same reason the Tauri packages are: an
   editor in the renderer is not something to let float.

2. **No DOMPurify.** Squire's default `sanitizeToDOMFragment` calls a global
   `DOMPurify` and throws without one. Instead it is given
   `richtext.ts`'s `sanitizeToFragment`, so every paste and every `setHTML`
   goes through the same whitelist as the message. There is one filter, not a
   paste sanitiser and a send sanitiser that can disagree, and what the user
   sees while writing is what goes out.

3. **Parsed twice, imported once.** `sanitizeToFragment` parses into an inert
   `<template>`, walks it, and imports a *second* parse of the walk's output.
   The first parse never enters the document: adopting an `<img onerror>` into
   the live document is enough to run it.

4. **The whitelist gains `<span>` and a rebuilt `style` attribute.** A `style`
   is never copied. Each property is read through the browser's CSS parser and
   then checked, and only what passes is written back:

   | Property | On | Accepted |
   |---|---|---|
   | `color`, `background-color` | `span` | `rgb()`/`rgba()` with only digits and separators inside, `#hex`, or a bare keyword; not `initial`/`inherit`/`unset`/`revert` |
   | `font-family` | `span` | the first family, if it is `sans-serif`, `serif` or `monospace` |
   | `font-size` | `span` | `small`, `large` or `x-large` |
   | `text-align` | `p`, `h1`–`h3`, `li`, `blockquote`, `pre` | `left`, `center`, `right`, `justify` |

   A span with no surviving property is unwrapped, keeping its text, as before.
   The toolbar's menus are built from the same lists the filter checks, so it
   cannot offer a style that would be stripped on send.

5. **`class` never reaches the wire.** Squire finds its own formatting by class
   (`color`, `highlight`, `font`, `size`, `align-*`), so in the editor's
   direction the filter writes the class back, *derived from which property
   survived*, never copied from the input. In the wire direction there is none.

6. **Everything else from 0025 stands.** `text/plain` is always sent and comes
   from the same walk; a body with nothing but `<p>` and `<br>` is sent as plain
   mail; links are `http`, `https` or `mailto`, parsed with no base URL.

## Consequences

- **eeemail now has a rich-text library in the process that renders untrusted
  mail**, which is what 0025 and [0013](0013-desktop-ui.md) declined. It
  never sees received mail: message HTML still renders only in the sandboxed
  frame with `default-src 'none'`, and Squire runs only on the composer's own
  element. Its exposure is to what the user pastes, which reaches it only
  through our filter.
- **Squire's defaults are not all ours.** Its subscript and superscript
  shortcuts are unbound, because a shortcut that formats text and then loses the
  formatting on send is worse than one that does nothing. A pasted
  `font-weight: bold` span (Google Docs, for one) loses its boldness; the text
  survives.
- **The sanitiser has no automated test.** The frontend has no test runner, and
  the checks run for this change were a throwaway bundle driven in headless
  Chromium. This is the most security-relevant code in the frontend, which makes
  that gap worth closing before it grows.
- **A recipient may not show what we sent.** Delta Chat's clients converged on
  the tag list without `span`; a client that ignores styles shows the words
  unstyled. That is the right way to fail, and the plain-text part never carried
  presentation anyway.
- eeemail now sends a little more HTML than 0025 allowed, and still less than
  its reading pane is willing to receive.

## Alternatives considered

**Keep `execCommand` and add the new commands.** No dependency, and the
serialiser would protect the wire either way. Rejected because the editing
behaviour, not the output, is what was wrong, and `foreColor`/`fontSize` are
the least consistent commands of all — `fontSize` still emits `<font size=…>`.

**Squire with DOMPurify.** What Squire's documentation suggests. Rejected
because DOMPurify's job is to make HTML *safe*, and ours is to make it *ours*: a
DOMPurify-clean paste still carries tables, images and arbitrary styles, which
the send filter would then strip, so what the user saw and what went out would
differ. A second dependency to arrive at a worse result.

**ProseMirror, TipTap, Quill, Lexical.** Schema-driven editors that would make
the whitelist structural rather than a filter. Each is several times Squire's
size, and several are built around a framework 0013 declined; the schema they
offer is one `richtext.ts` already enforces in under four hundred lines.

**Unrestricted colours, fonts and sizes.** Rejected for font family and size: a
free-text family is the one style value that could carry anything, a named font
is a guess about what the recipient has installed, and pixel sizes do not scale
with the reader's default. Colours are unrestricted in value because the check
leaves nothing in them but a colour.
