# Out-of-scope upstream features: removal inventory

`core/` is a fork of `chatmail/core`, which is a chat engine. Several of its
features have no place in an email client. This file inventories them, the exact
sites that hold them in, and the order to remove them in.

**We gate before we delete.** Group machinery, webxdc and peer channels are
woven through `receive_imf/` and `chat/`; deleting them in Phase 0 means
fighting the compiler instead of building
([ADR 0001](adr/0001-fork-chatmail-core.md)). Each feature below is a cargo
feature in `core/Cargo.toml`, currently in `default` so the fork builds and
tests identically to upstream. Removal means adding `#[cfg(feature = "...")]`
at the listed sites, dropping the feature from `default`, and only then deleting
the module.

Site counts and line numbers are against our fork point, **`v2.59.0`**
(`e322fdf`). Expect drift on merges; re-measure rather than trusting the numbers.

---

## `relay-provisioning` — chatmail relay account provisioning  ✅ **gated off**

Relay selection/migration plus the `DCACCOUNT:` QR scheme that hands out an
account from a scanned code. eeemail uses traditional `user@domain` accounts
([ADR 0007](adr/0007-server-template.md)).

Upstream renamed this module to `autorelay.rs` after our fork point; expect the
paths to move on the next merge.

| Site | What |
|---|---|
| `core/src/automatic_relay_management.rs` | The module itself (plus its `_tests.rs`) |
| `core/src/lib.rs:58` | `mod automatic_relay_management;` |
| `core/src/qr.rs:14` | `use ...::login_param_from_host` |
| `core/src/qr.rs:821-830` | `login_param_from_account_qr` — the `DCACCOUNT:` handler |
| `core/src/qr.rs:375` | `DCACCOUNT_SCHEME` dispatch arm |
| `core/src/imap/idle.rs:57` | `spawn(maybe_add_additional_relays(..))` before IDLE |
| `core/src/qr/qr_tests.rs` | DCACCOUNT tests must be gated alongside |

`core/src/context.rs:1053-1068` also emits three `automatic_relay_management*`
keys in the info map. These only read `Config` values and are harmless; leave
them until the module goes.

**Status:** gated off (removed from `default` in `core/Cargo.toml`). Rather than
gating the `Qr::Account` enum variant and every `match` arm on it, the two
implementation functions are feature-gated and paired with `#[cfg(not(...))]`
stubs that `bail!` with a clear message. A `DCACCOUNT:` QR therefore fails
cleanly instead of silently doing nothing, and no enum definition or match arm
carries a `cfg` -- far less to conflict on at merge time. Exact patches are in
[`fork-patches.md`](fork-patches.md).

Still to do: delete `automatic_relay_management.rs` and the `DCACCOUNT:`
dispatch entirely once our layer is stable.

**Note:** this removes relay *provisioning* only. A chatmail relay remains a
perfectly good IMAP/SMTP transport and must keep working as one.

## `peer-channels` — iroh peer-to-peer realtime channels

The realtime transport behind webxdc and decentralized groups/channels.

| Site | What |
|---|---|
| `core/src/peer_channels.rs` | The module itself |
| `core/src/lib.rs:119` | `pub mod peer_channels;` |
| `core/src/context.rs:28,317-318` | `use ...::Iroh`; the `iroh` field on `Context` |
| `core/src/mimefactory.rs:34,857,2158` | `get_iroh_topic_for_msg`, `create_iroh_header` on send |
| `core/src/receive_imf.rs:42,2038,2275-2276,2482-2483` | Gossip peer + topic stubs on receive |

Gating the send and receive sites needs a decided fallback, not just a `#[cfg]`:
outgoing mail simply omits the iroh header, and incoming iroh headers are
ignored rather than stubbed. Both are safe — they degrade to ordinary email.

## `webxdc` — mini-apps

Chat-app feature with no email-client analogue. Depends on `peer-channels` for
realtime, so `webxdc = ["peer-channels"]`.

~35 files reference it, with 12 sites in `chat.rs`/`chat/` and 5 in
`receive_imf`. **This is the coupled one.** Do it last, after our email layer is
stable, and expect it to be a real piece of work rather than a mechanical gate.

## Not gated — deliberately kept

| Feature | Why we keep it |
|---|---|
| `calls.rs` | WebRTC with STUN/TURN/ICE, resolved from IMAP `METADATA`. Wanted for later device sync and video meetings. |
| `securejoin/` | The reason we forked. |
| `ephemeral.rs`, MDN | On by default in eeemail ([ADR 0006](adr/0006-encryption-policy.md)). |
| `imex/` | Encrypted backup — a correctness requirement once the server is only a spool ([ADR 0003](adr/0003-imap-as-transport.md)). |
| SQLCipher wiring in `sql.rs` | Already present; at-rest encryption becomes a key-management decision, not an engine change ([ADR 0004](adr/0004-local-store-and-raw-mime.md)). |

## Verified groups

`vg-*` SecureJoin and the group membership protocol are out of scope, but they
share code paths with Setup-Contact (`vc-*`), which we very much want. This is
**not** a cargo feature: it needs to be untangled by hand in the email layer
rather than gated. Deferred until after Phase 2, when recipients are decoupled
from chat membership and the shape of the problem is clearer.
