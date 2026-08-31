# 0002 — License the project under MPL-2.0

**Status:** Accepted — 2026-08-31 (supersedes the initial Unlicense)

## Context

The repository was created under the Unlicense (public domain dedication).
`chatmail/core` is MPL-2.0, which is file-level copyleft: modifications to
MPL-licensed files must remain MPL-licensed, though the license does not extend
to separate files merely linked against them.

Having decided to fork core ([0001](0001-fork-chatmail-core.md)), continuing
under the Unlicense is not an option for the forked tree.

## Decision

Relicense the project to MPL-2.0, matching upstream.

## Consequences

- We can copy and adapt any `chatmail/core` source file without a license
  boundary to reason about — including, if useful, lifting whole modules rather
  than reimplementing from spec.
- Contributions are under MPL-2.0. The prior Unlicense dedication covered only
  the initial commit's `README` and `LICENSE`; there is no third-party
  contribution to re-license.
- Downstream users get file-level copyleft: they may combine eeemail with
  proprietary code, but changes to our files must be published.
- Bundled dependencies keep their own licenses. `NOTICE` records them —
  notably rPGP, `async-imap` and `async-smtp` (MIT/Apache-2.0), and
  `deltachat-contact-tools` and `format-flowed` (MPL-2.0).
