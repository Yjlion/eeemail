# 0012 — eeemail's defaults are applied at setup, not as compile-time defaults

**Status:** Accepted — 2026-09-01

## Context

[0006](0006-encryption-policy.md) settled that eeemail is opportunistic by
default. Upstream declares `ForceEncryption` with `#[strum(props(default =
"1"))]` — strict — because upstream ships for chatmail relays, where everything
is encrypted by definition.

Changing that one character to `"0"` makes 22 upstream tests fail. They are not
wrong: they assert upstream's security policy, which we deliberately changed. But
patching 22 test sites means carrying 22 conflict points into every future merge,
forever, for a value that only has to be written once.

Separately, Phase 6 needed somewhere to put the RPC surface and the CLI, and the
plan's tree puts `rpc/` and `cli/` beside `core/`.

## Decision

**eeemail's defaults are applied at account setup**, by
`email::policy::apply_defaults`, not by changing upstream's compile-time
defaults. Every eeemail entry point calls it: the CLI on open, and the
`applyEeemailDefaults` RPC method.

**The RPC methods live in a marked block inside upstream's `impl CommandApi`**,
with the types in a new `api/types/email.rs`.

**The CLI is a top-level `cli/` crate that is a member of the `core/`
workspace.**

## Consequences

- `apply_defaults` has two rules that make it safe. It **never touches a
  configured account**: an existing Delta Chat profile opened with eeemail keeps
  its settings, which matters because `ForceEncryption` is device-synced and
  writing it would push a weaker policy to the user's other clients. And it
  **never overwrites an explicit choice**, only values never set.
- The cost is that a bare `Context` opened by something other than eeemail keeps
  upstream's strict default. That is why every entry point must call
  `apply_defaults`, and why Phase 7's account setup must too. This is the one
  place where forgetting a call produces a wrong default rather than a
  compile error.
- Upstream test churn from this decision is **zero**.
- The RPC methods cannot live in their own file: `yerpc`'s `#[rpc]` attribute
  generates one trait implementation, so a second `impl CommandApi` block would
  conflict. They are kept thin — every one is a direct call into
  `deltachat::email::*` — so a merge conflict is resolved by re-placing the
  block, not by reading it.
- Threads are returned **flat**, as `(msgId, parentMsgId, depth)` in display
  order, not as a nested tree. `typescript_type_def` cannot express a directly
  recursive type, and a flat list is what a threaded reading pane renders
  anyway; it also cannot blow the JSON nesting limit on a pathologically deep
  reply chain.
- `cli/` sits outside `core/` so the fork stays the fork, but names `core/` as
  its workspace root (`workspace = "../core"`), so it shares one target
  directory and one lockfile rather than compiling `deltachat` a second time.
  The plan's separate `rpc/` crate was not created: the RPC surface has to be in
  `deltachat-jsonrpc` for the `#[rpc]` reason above, so a wrapper crate would
  add a build unit and no separation.
