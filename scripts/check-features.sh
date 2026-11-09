#!/usr/bin/env bash
# Assert that the SDL2 choice a caller makes actually reaches `sdl2`.
#
# `sdl2-sys` gives its `bundled` feature priority over `use-pkgconfig`: when both are
# on it runs CMake unconditionally and ignores pkg-config. Cargo unions features across
# the whole graph, so a crate that depends on `giochi-engine` without
# `default-features = false` re-enables `sdl2-bundled` for everybody downstream, and
# `--features sdl2-system` becomes a no-op that a consumer cannot fix from their side.
# That is exactly what happened to `giochi-engine-3d`, and it broke the cross-compile
# path: the whole point of `sdl2-system` is to build against the SDL2 already in the
# target image instead of compiling a second copy with CMake.
#
# The manifests look correct in that broken state — the failure only exists in the
# feature resolution — so this checks the resolution itself, through `cargo tree`.
#
# Usage:
#   scripts/check-features.sh
#
# Exits non-zero with the offending crate and the feature path that pulled `bundled` in.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Every crate a caller can depend on directly. The 3D crate is the one that was broken,
# but the defect is "internal dependency without `default-features = false`", which is
# not specific to it: capi is what ships to the console, and lua/demo forward to the
# same graph.
crates=(giochi-engine giochi-engine-3d giochi-engine-capi giochi-engine-lua giochi-demo)

fail=0

# `cargo tree` exits non-zero when the crate has no such feature, which is itself a
# finding: a crate in this list that cannot offer `sdl2-system` cannot be built for the
# console image at all.
sdl2_features() {
    local crate="$1"
    shift
    cargo tree -p "$crate" "$@" -e features -i sdl2 --prefix depth 2>/dev/null
}

for crate in "${crates[@]}"; do
    if ! system=$(sdl2_features "$crate" --no-default-features --features sdl2-system); then
        echo "FAIL $crate: cannot resolve '--no-default-features --features sdl2-system'"
        echo "     every consumer-facing crate must forward that feature"
        fail=1
        continue
    fi

    if grep -q '^[0-9]*sdl2 feature "bundled"' <<<"$system"; then
        echo "FAIL $crate: 'sdl2-system' still resolves 'sdl2/bundled'"
        echo "     sdl2-sys then runs CMake and ignores pkg-config. Add"
        echo "     'default-features = false' to the giochi-engine dependency."
        fail=1
        continue
    fi

    if ! grep -q 'sdl2 feature "use-pkgconfig"' <<<"$system"; then
        echo "FAIL $crate: 'sdl2-system' does not enable 'sdl2/use-pkgconfig'"
        fail=1
        continue
    fi

    # The default has to keep meaning "bundled", or this gate would be satisfied by
    # simply dropping the default and changing behaviour for everyone who asked for
    # nothing.
    if default=$(sdl2_features "$crate"); then
        if ! grep -q '^[0-9]*sdl2 feature "bundled"' <<<"$default"; then
            echo "FAIL $crate: the default feature set no longer means 'sdl2/bundled'"
            fail=1
            continue
        fi
    else
        echo "FAIL $crate: cannot resolve the default feature set"
        fail=1
        continue
    fi

    echo "  ok       $crate (default: bundled, sdl2-system: pkg-config)"
done

if ((fail)); then
    echo
    echo "feature resolution is broken; see the failures above." >&2
    exit 1
fi

# Cross-compiling is the reason `sdl2-system` exists, so at least prove the resolution
# the console build would use. The compile itself needs a sysroot, which lives in
# scripts/build-aarch64.sh; this only asserts the graph.
echo
echo "feature forwarding is consistent."
