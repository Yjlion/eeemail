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
