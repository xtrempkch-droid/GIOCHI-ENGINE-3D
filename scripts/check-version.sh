#!/usr/bin/env bash
# Verify that the version declared in Cargo.toml is echoed everywhere it is written
# down. No argument: report and exit non-zero on any mismatch.
#
# A bump touches six places, and forgetting one produces a release whose artefacts
# disagree about what they are -- a header claiming 0.1.0 while the library reports
# 0.1.1, or a Lua binding whose `giochi.version` is a release behind. The Rust
# constants are already tied to `CARGO_PKG_VERSION` by a unit test
# (`version_constants_match_package_version`); this script covers the files the
# compiler cannot see.
#
#   scripts/check-version.sh
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

manifest="Cargo.toml"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$manifest" | head -1)"

if [[ -z "$version" ]]; then
    echo "error: could not read the workspace version from $manifest" >&2
    exit 1
fi

echo "==> workspace version: $version"

failures=0
fail() {
    echo "  MISMATCH $1" >&2
    echo "           expected to mention $version" >&2
    failures=$((failures + 1))
}

# Every file below must state the version literally somewhere.
expect() {
    local file="$1" needle="$2"
    if [[ ! -f "$file" ]]; then
        echo "  MISSING  $file" >&2
        failures=$((failures + 1))
        return
    fi
    if grep -qF "$needle" "$file"; then
        echo "  ok       $file"
    else
        echo "  STALE    $file (looking for '$needle')" >&2
        failures=$((failures + 1))
    fi
}

# The member manifests pin the path dependency to the workspace version, so a
# published crate cannot resolve to a version that was never released. A member may
# either inherit (`version.workspace = true`) or restate the literal.
for crate in giochi-engine giochi-engine-capi giochi-engine-lua giochi-demo; do
    file="$crate/Cargo.toml"
    if [[ ! -f "$file" ]]; then
        echo "  MISSING  $file" >&2
        failures=$((failures + 1))
    elif grep -qF 'version.workspace = true' "$file"; then
        echo "  ok       $file (inherits)"
    elif grep -qF "version = \"$version\"" "$file"; then
        echo "  ok       $file"
    else
        fail "$file"
    fi
done

# A `{ path = ... , version = "X" }` pin that lags the workspace publishes a crate
# whose dependency can never be satisfied.
for file in giochi-engine-capi/Cargo.toml giochi-engine-lua/Cargo.toml giochi-demo/Cargo.toml; do
    while IFS= read -r pinned; do
        if [[ "$pinned" != "$version" ]]; then
            echo "  STALE    $file pins a path dependency to $pinned" >&2
            failures=$((failures + 1))
        fi
    done < <(sed -n 's/.*path *= *"[^"]*", *version *= *"\([^"]*\)".*/\1/p' "$file")
done

expect giochi-engine/src/lib.rs "pub const VERSION_PATCH: u32 = ${version##*.};"
expect docs/C_ABI_CONTRACT.md "**Engine version: \`$version\`**"
expect docs/LUA_BINDING.md "(\`$version\`)"
expect README.md "**Status: $version"

# The header is generated, but its doc comments carry the packed example.
expect giochi-engine-capi/include/giochi.h "For version \`$version\`"

# A stale pre-release note is the classic one: the changelog keeps an [Unreleased]
# section while the tag is cut.
if grep -qF "## [$version]" CHANGELOG.md; then
    echo "  ok       CHANGELOG.md"
else
    echo "  STALE    CHANGELOG.md (no '## [$version]' section)" >&2
    failures=$((failures + 1))
fi

# Cargo.lock must agree or `--locked` builds fail for consumers.
if grep -q "name = \"giochi-engine\"" Cargo.lock; then
    if awk -v want="$version" '
        /^name = / { name = $3 }
        name == "\"giochi-engine\"" && /^version = / {
            gsub(/"/, "", $3)
            if ($3 == want) { found = 1 }
            name = ""
        }
        END { exit found ? 0 : 1 }
    ' Cargo.lock; then
        echo "  ok       Cargo.lock"
    else
        echo "  STALE    Cargo.lock (run cargo build to refresh it)" >&2
        failures=$((failures + 1))
    fi
fi

echo
if [[ "$failures" -gt 0 ]]; then
    echo "$failures version reference(s) out of date" >&2
    exit 1
fi
echo "version $version is consistent."
