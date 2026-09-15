# 0031 — Release artefacts are built on main and published from the tag

**Status:** Accepted — 2026-09-15

## Context

Through v0.6.0, pushing a `v*` tag started the whole release build, and nothing
else. v0.6.0's run took 37 minutes: 17 on Linux and 36 on Windows. Three things
made it that slow, and none of them was the code:

- **`deltachat` compiled twice in each leg.** `cargo build -p eeemail-cli -p
  deltachat-rpc-server` and `tauri build` are separate cargo invocations. They
  unify dependency features differently, so the second invocation cannot reuse
  the first's `deltachat`. They also ran one after the other.
- **Every link was fat LTO over one codegen unit** (`core/Cargo.toml`'s
  `[profile.release]`, which is upstream's). On Windows, the headless binaries
  alone took 20 minutes.
- **The cache never helped.** The cache was saved under the tag's ref, and a
  tag can read only its own caches and the default branch's. Each release
  built cold, then spent two more minutes saving a cache that nothing would
  read.

On top of the build, the process had a rule someone had to remember: tag only
after CI passes on the merge commit. Merge the release PR, wait for CI on main,
tag, wait again. A release PR merged with CI still running took a placeholder
line onto main and cost a second PR (#36) before the tag could go on.

## Decision

**Every push to `main` builds the release artefacts. A tag publishes the
artefacts already built for its commit, once CI has passed on that commit.**

1. **`release.yml` runs on pushes to `main`**, not only on tags. It builds
   `{linux, windows} × {app, tools}` as four parallel jobs. `app` is `tauri
   build` plus its installers; `tools` is `eeemail-cli` and
   `deltachat-rpc-server`. A `package` job per platform then assembles exactly
   the archive, installers and checksums the release used to publish. A newer
   push cancels an older prebuild.
2. **A tag waits instead of starting over.** `reuse` finds main's prebuild of
   the tagged commit and waits for it if it is still running. `gate` waits for
   `ci.yml` to finish on that commit and fails unless it passed. `release`
   downloads the prebuild's packages and publishes them. If there is no usable
   prebuild (none exists, it failed or was cancelled, or its artefacts have
   expired), the tag builds for itself, the same way main does.
3. **Thin LTO with 16 codegen units**, set through
   `CARGO_PROFILE_RELEASE_LTO` and `CARGO_PROFILE_RELEASE_CODEGEN_UNITS` in
   the workflow. `core/Cargo.toml` is untouched, so the fork's profile stays
   upstream's and the fork-patch ledger does not grow.
4. **Rust caches are saved from `main` only**, in both workflows. PRs and tags
   restore main's caches, and no longer write duplicates that push the
   repository past its 10 GB cache limit and evict the caches that matter.

## Consequences

- **The tag can go on the moment the release PR is merged**, and the release
  publishes by itself about a minute after main's CI and prebuild both finish.
  "CI before the tag" is enforced by the workflow instead of remembered.
- **The release notes are still read from the tagged commit.** They must be
  final when the release PR opens, because nothing publishes them any later.
- **Binaries are somewhat larger** than under fat LTO. For a mail client that
  spends its time waiting on the network, this trade is worth a release that
  does not take forty minutes.
- **Main spends runner time on every push.** The repository is public, so the
  cost is queue time rather than money. `workflow_dispatch` still builds
  without publishing and remains the way to rehearse from a branch.
- **A tag on a commit that is not on `main` does not publish.** `gate` finds no
  CI run for it and says so after ten minutes. That is the intended behaviour.
- **CI on `main` now matters to publishing.** `ci.yml` cancels an in-progress
  run when a newer push lands. If a release commit is superseded before its CI
  finishes, re-run that CI, then re-run the release's failed jobs.
