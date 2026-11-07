#!/usr/bin/env bash
# Regenerate (or verify) giochi-engine-capi/include/giochi.h from the Rust source.
#
#   scripts/gen-header.sh          # rewrite the header
#   scripts/gen-header.sh --check  # fail if the committed header is stale
#
# The header is a build artefact and must never be edited by hand: the ABI is
# specified in docs/C_ABI_CONTRACT.md, the Rust `#[repr(C)]` types are its only
# implementation, and cbindgen is the only thing allowed to turn one into the other.
#
# Requires cbindgen 0.27 (installed by .github/workflows/ci.yml):
#   cargo install cbindgen --version 0.27.0 --locked
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

config="giochi-engine-capi/cbindgen.toml"
committed="giochi-engine-capi/include/giochi.h"

check_only=0
if [[ "${1:-}" == "--check" ]]; then
    check_only=1
fi

if ! command -v cbindgen >/dev/null 2>&1; then
    echo "error: cbindgen not found." >&2
    echo "       cargo install cbindgen --version 0.27.0 --locked" >&2
    exit 1
fi

generated="$(mktemp)"
trap 'rm -f "$generated"' EXIT

# cbindgen warns about generic types it cannot express in C (they live in private
# fields of the engine's registries and are never exported). The header itself is
# unaffected, so the warnings are noise here.
if ! cbindgen --config "$config" --crate giochi-engine-capi --output "$generated" 2>/dev/null; then
    echo "error: cbindgen failed" >&2
    exit 1
fi

# The generator stamps its own version into the header. Diffing that would make the
# check fail on a harmless cbindgen upgrade, so it is stripped from both sides.
strip_version() {
    grep -v '^/\* Generated with cbindgen:' "$1"
}

if [[ "$check_only" == "1" ]]; then
    if ! diff -u <(strip_version "$committed") <(strip_version "$generated") > /tmp/giochi-header.diff; then
        echo "error: $committed is stale." >&2
        echo "       run scripts/gen-header.sh and commit the result." >&2
        echo >&2
        head -60 /tmp/giochi-header.diff >&2
        rm -f /tmp/giochi-header.diff
        exit 1
    fi
    rm -f /tmp/giochi-header.diff
    echo "$committed is up to date."
else
    mkdir -p "$(dirname "$committed")"
    cp "$generated" "$committed"
    echo "wrote $committed ($(wc -l < "$committed") lines)"
fi
