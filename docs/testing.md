# Testing the fork

## Use `cargo nextest`, not `cargo test`

```sh
cd core && cargo nextest run --workspace
```

This is what upstream's own CI does (`core/.github/workflows/ci.yml`), and it is
not a stylistic preference — `cargo test` **fails** on a green tree.

## Why

`deltachat-time` mocks the clock through a **process-global** static:

```rust
// core/deltachat-time/src/lib.rs
static SYSTEM_TIME_SHIFT: RwLock<Duration> = RwLock::new(Duration::new(0, 0));
pub fn shift(duration: Duration) { *SYSTEM_TIME_SHIFT.write().unwrap() += duration; }
```

`SystemTime::shift()` moves the clock for every test sharing the process, and
the shift **accumulates** — nothing resets it between tests. There are 107 call
sites across 21 files.

`cargo test` runs the whole suite in one process, so it loses either way:

| Runner | Result at `v2.59.0`, 3-core machine |
|---|---|
| `cargo test --workspace` | 4 failed / 1110 passed (concurrent interference) |
| `cargo test --workspace -- --test-threads=1` | 2 failed / 1112 passed (sequential, but drift still accumulates) |
| `cargo nextest run --workspace` | **1153 passed, 0 failed** (145s) |

`--test-threads=1` only removes *concurrent* interference. It does nothing about
accumulated drift, which is why two tests still fail: by the time they run, the
global clock has been shifted hours into the future by earlier tests.

nextest runs **each test in its own process**, so the global static starts at
zero every time. That is a real fix rather than a mitigation, and it is also
faster (145s vs 363s single-threaded) because it still parallelizes.

The tests that expose this — all of which pass in isolation — are
`tools_tests::test_maybe_warn_on_outdated`,
`blob_tests::test_create_and_deduplicate_from_bytes`,
`location::tests::test_delete_expired_locations` and
`receive_imf_tests::test_dont_verify_by_verified_by_unknown`.

This is upstream behavior, not something our fork introduced: it reproduces on a
pristine `v2.59.0` checkout, and the failure set is identical with and without
our `Cargo.toml` feature additions.

**Do not "fix" these tests in the fork.** They are correct; the harness is
shared mutable state, and upstream has already solved it at the runner level.

## Doctests

nextest does not run doctests. Upstream runs them separately, and so do we:

```sh
cargo test --workspace --locked --doc
```

## When adding our own tests

Anything in `core/src/email/` that reads the clock should take timestamps as
parameters rather than calling `SystemTime::now()` internally. That makes tests
deterministic regardless of runner, and is better design besides.

## The live passes

Three Python scripts test things a unit test cannot, because they need a real
IMAP/SMTP server, a second process, or a second OpenPGP implementation. None of
them runs in CI yet — see the constraint in [`handoff.md`](handoff.md) — so they
are run by hand, and they are the part of the suite a merge from upstream most
needs.

All three need the test mail server up, and need it brought up from the compose
file rather than the bare `docker run` in [`../server/README.md`](../server/README.md):
that recipe passes no `ACCOUNTS` and provisions only `alice` and `bob`.

```sh
cd server/compose && docker compose up -d --build && python3 smoke-test.py
cd ../.. && cd core && cargo build -p deltachat-rpc-server && cd ..
```

| Script | What it proves | Needs |
|---|---|---|
| [`../scripts/e2e-pass.py`](../scripts/e2e-pass.py) | eeemail works end to end against a real server: setup, a Cc'd message with an attachment, gating, a recoverable expiry, encryption at rest. Both sides are our own core, so it proves **no** interop. | the server |
| [`../scripts/interop-pass.py`](../scripts/interop-pass.py) | Autocrypt and SecureJoin against upstream's released `deltachat-rpc-server` — the same binary Delta Chat Desktop ships. | the server, and the pinned release (downloaded and hash-checked) |
| [`../scripts/gpg-interop-pass.py`](../scripts/gpg-interop-pass.py) | our outgoing PGP/MIME and signatures are readable by GnuPG, an OpenPGP implementation sharing no code with rPGP. | the server, and `gpg` |

Run them on the **permissive** container. The `STRICT_E2EE=1` profile rewrites
`Subject` at submission, and all three assert on subjects.

Each script prints one `PASS` line per assertion and exits non-zero with a
numbered list if anything failed. `--keep` leaves the temporary accounts
directories behind for inspection, and `--log RUST_LOG` lets the servers write
to stderr.

**A green unit-test run does not mean these pass.** That is the entire reason
they exist: upstream changes crypto and protocol code routinely, and
`e2e-pass.py` running the same core on both sides cannot see it.
