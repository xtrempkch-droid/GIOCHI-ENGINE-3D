#!/usr/bin/env bash
# Run every quality gate required by PROGRESS.md.
#
# Usage:
#   scripts/gates.sh              # all gates
#   SKIP_ARM64=1 scripts/gates.sh # skip the aarch64 cross-check
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    echo
    echo "==> $*"
    "$@"
}

run cargo fmt --all --check
run cargo clippy --workspace --all-targets -- -D warnings
run cargo build --workspace --target x86_64-unknown-linux-gnu
run cargo test --workspace --target x86_64-unknown-linux-gnu
run cargo doc --workspace --no-deps --document-private-items

if [[ "${SKIP_ARM64:-0}" == "1" ]]; then
    echo
    echo "==> aarch64 cross-check SKIPPED (SKIP_ARM64=1)"
else
    run scripts/check-aarch64.sh
fi

echo
echo "All gates passed."
