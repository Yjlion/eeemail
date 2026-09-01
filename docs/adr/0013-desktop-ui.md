# 0013 — The desktop shell is a JSON-RPC pipe, and message content never runs in the app document

**Status:** Accepted — 2026-09-01

## Context

Phase 7 needs a desktop client over the RPC surface from
[0012](0012-rpc-and-cli.md). Two questions had to be answered before writing any
UI.

**How does the frontend reach the engine?** Tauri's idiomatic answer is a
`#[tauri::command]` per operation. The RPC surface is a couple of hundred
methods and grows every phase.

**How is message content rendered?** Every message body in the reading pane came
from a stranger, and this is a client whose entire premise is that mail is
private.

## Decision

**One transport, not a command per method.** The shell exposes a single
`rpc_send` command and emits everything coming back — responses *and* engine
events — as an `rpc-message` event. This is the shape `deltachat-rpc-server`
already has over stdin/stdout, with Tauri's IPC substituted.

**Message content never runs in the app document.** The window CSP allows no
remote origins at all. HTML mail is rendered inside an iframe with a bare
`sandbox` attribute — no `allow-*` tokens — so it has no scripts, no forms, no
navigation and a `null` origin.

## Consequences

- Mirroring each RPC method as a Tauri command would mean writing every
  signature three times: Rust, the handler list, and TypeScript. The generated
  TypeScript client in `deltachat-jsonrpc/typescript/generated` already exists
  and is type-checked. One pipe means adding an RPC method costs nothing on the
  shell side.
- Responses are **not** returned from `rpc_send`. The JSON-RPC session delivers
  them asynchronously on its outbound channel, so the frontend has one inbound
  path to handle rather than two. `rpc_send` also spawns rather than awaits, so
  a long-running call such as a fetch does not stall every request behind it.
- **Two independent barriers against message content reaching the network**, and
  that redundancy is the point: the window CSP forbids remote origins, *and*
  the renderer strips remote references and reports that it did. A single remote
  image is a read receipt the sender gets whether the user consented or not,
  plus the user's IP address — so a mistake in either layer alone must not be a
  privacy breach.
- Links in message bodies render as text rather than anchors. Opening an
  external link is a deliberate action the shell should mediate, not something
  a message decides.
- The UI shows engine state as it is rather than smoothing it over: whether a
  message was encrypted, whether the contact is *verified* (the only property
  that survives an active attacker, and so the only one shown as a claim about
  identity), who a message was addressed to but never delivered to, and whether
  the original source has expired. An encrypted mail client that hides these is
  worse than one that has none of them, because the user cannot tell which
  messages were protected.
- The frontend is plain DOM with no framework. It is small enough not to need
  one, and a framework would be a large dependency surface in the process that
  handles decrypted mail.
- `desktop/src/types.ts` hand-writes only the slice of the RPC surface the UI
  consumes, rather than importing the ~200-method generated bindings. It makes
  it visible when the UI starts depending on something new; the generated
  bindings stay the source of truth for shapes.
