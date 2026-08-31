#!/usr/bin/env bash
#
# Enforces the fork discipline in docs/adr/0001-fork-chatmail-core.md.
#
# core/ is a fork of chatmail/core. Every upstream file we patch is a file that
# will conflict on some future merge, and whoever resolves that conflict will
# not be us. So each one must carry a recorded reason in docs/fork-patches.md.
#
# Files under core/src/email/ are ours, not upstream's, and are exempt.
#
# Passes when the fork is untouched, which is the desired steady state.

set -euo pipefail

FORK_BASE_FILE="docs/fork-base"
LEDGER="docs/fork-patches.md"
EXEMPT_PREFIX="core/src/email/"

[ -f "$FORK_BASE_FILE" ] || { echo "missing $FORK_BASE_FILE"; exit 1; }
[ -f "$LEDGER" ] || { echo "missing $LEDGER"; exit 1; }

fork_base="$(tr -d '[:space:]' < "$FORK_BASE_FILE")"

if ! git cat-file -e "${fork_base}^{commit}" 2>/dev/null; then
  echo "note: upstream commit $fork_base not present locally; fetching."
  git fetch --no-tags --quiet \
    https://github.com/chatmail/core.git "$fork_base" || {
      echo "could not fetch upstream commit $fork_base" >&2; exit 1; }
  fork_base=FETCH_HEAD
fi

# Build a tree from the *working* copy, not HEAD, so an uncommitted patch is
# still reported. In CI these are identical; locally they are not, and a check
# that silently passes on uncommitted work is worse than no check.
tmp_index="$(mktemp)"
trap 'rm -f "$tmp_index"' EXIT
GIT_INDEX_FILE="$tmp_index" git read-tree HEAD
GIT_INDEX_FILE="$tmp_index" git add -A -- core
work_tree="$(GIT_INDEX_FILE="$tmp_index" git write-tree)"

# Compare our core/ subtree against the upstream tree it was forked from.
# Paths from this diff are upstream-relative, so re-prefix them with core/.
mapfile -t changed < <(
  git diff --name-only "${fork_base}:" "${work_tree}:core" \
    | sed 's|^|core/|' \
    | grep -v "^${EXEMPT_PREFIX}" \
    || true
)

if [ ${#changed[@]} -eq 0 ]; then
  echo "Fork is unmodified against upstream $fork_base. Nothing to record."
  exit 0
fi

undocumented=()
for f in "${changed[@]}"; do
  grep -qF -- "$f" "$LEDGER" || undocumented+=("$f")
done

echo "Patched upstream files (${#changed[@]}):"
printf '  %s\n' "${changed[@]}"

if [ ${#undocumented[@]} -gt 0 ]; then
  cat >&2 <<EOF

FAIL: these upstream files are patched but absent from $LEDGER:

$(printf '  %s\n' "${undocumented[@]}")

Add a row to the "Patches to upstream files" table for each, saying what the
change is and why. See docs/adr/0001-fork-chatmail-core.md.
EOF
  exit 1
fi

echo "All patched files are recorded in $LEDGER."
