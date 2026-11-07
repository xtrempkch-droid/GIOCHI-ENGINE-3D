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

if command -v cbindgen >/dev/null 2>&1; then
    run scripts/gen-header.sh --check
else
    echo
    echo "==> C header check SKIPPED (cbindgen not installed)"
    echo "    cargo install cbindgen --version 0.27.0 --locked"
fi

# Version fields live in six files the compiler cannot check against each other;
# a release that disagrees with its own header is the failure mode (ADR: see
# CONTRIBUTING, "Releasing").
run scripts/check-version.sh

# The BMP twins exist so a target without a PNG decoder can still load the assets
# (ADR-036). Byte comparison, because a same-size twin with different pixels would
# pass every other gate.
if command -v python3 >/dev/null 2>&1; then
    run python3 scripts/png-to-bmp.py --check assets/player.png assets/checker.png
else
    echo
    echo "==> asset twin check SKIPPED (python3 not installed)"
fi

# CI calls these scripts rather than reimplementing them, so a broken script fails
# every job at once. Linting them here catches that before the push.
if command -v shellcheck >/dev/null 2>&1; then
    run shellcheck scripts/*.sh
else
    echo
    echo "==> shellcheck SKIPPED (not installed)"
    echo "    sudo apt install shellcheck"
fi

if [[ "${SKIP_ARM64:-0}" == "1" ]]; then
    echo
    echo "==> aarch64 cross-check SKIPPED (SKIP_ARM64=1)"
else
    run scripts/check-aarch64.sh
fi

echo
echo "All gates passed."
