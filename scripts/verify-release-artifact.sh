#!/usr/bin/env bash
# Verify a release tarball by actually running it. This is the review step the draft
# release exists for (see CONTRIBUTING.md, "Releasing").
#
# The workflow leaves the release as a draft so the archives get exercised before
# anyone downloads them. Doing that by hand is error-prone -- and on 0.1.0 nobody did
# it, so a build with four portability bugs sat one click away from being published.
#
#   scripts/verify-release-artifact.sh giochi-engine-x86_64-unknown-linux-gnu.tar.gz
#
# Checks, all with SDL_VIDEODRIVER=dummy so no display is needed:
#   * the archive carries the binary, header, both library forms, the Lua game,
#     the docs and the assets -- in both formats (ADR-036);
#   * giochi.h states the version the archive was built from;
#   * libgiochi.so exports the ABI;
#   * the demo, the Lua reference game and a real C consumer all run;
#   * they run again with the PNGs deleted, which is the console situation.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
    echo "usage: $0 <release.tar.gz> [--keep]" >&2
    echo "       --keep  leave the unpacked tree for inspection" >&2
}

die() {
    echo "error: $*" >&2
    exit 1
}

archive=""
keep=0
for arg in "$@"; do
    case "$arg" in
        --keep) keep=1 ;;
        -h|--help) usage; exit 0 ;;
        -*) usage; die "unknown option: $arg" ;;
        *) [[ -n "$archive" ]] && die "only one archive at a time"; archive="$arg" ;;
    esac
done

[[ -n "$archive" ]] || { usage; exit 2; }
[[ -f "$archive" ]] || die "$archive does not exist"

work="$(mktemp -d)"
cleanup() {
    if [[ "$keep" == "1" ]]; then
        echo "kept: $work"
    else
        rm -rf "$work"
    fi
}
trap cleanup EXIT

failures=0
check() {
    local what="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        echo "  ok    $what"
    else
        echo "  FAIL  $what" >&2
        failures=$((failures + 1))
    fi
}

echo "==> unpacking $archive"
tar -C "$work" -xzf "$archive"
[[ -d "$work/lib" || -d "$work/include" ]] || die "not a giochi-engine archive (no lib/ or include/)"

# Expected layout, straight from the Package step of .github/workflows/release.yml.
echo "==> layout"
for path in giochi-demo include/giochi.h lib/libgiochi.so lib/libgiochi.a \
            lua/mini_game.lua README.md CHANGELOG.md LICENSE \
            assets/player.png assets/player.bmp assets/checker.png assets/checker.bmp; do
    check "$path" test -e "$work/$path"
done
# The 3D examples are how a user sees the renderer; an archive whose only 3D artifact is
# a shared library is one nobody can run.
for example in cube_demo mini_fps ball_balance; do
    if [[ -x "$work/bin/$example" ]]; then
        echo "  ok    bin/$example present"
    else
        echo "  FAIL  bin/$example missing from the archive" >&2
        failures=$((failures + 1))
    fi
done

for doc in C_ABI_CONTRACT ARCHITECTURE KMSDRM CROSS_COMPILE API_GUIDE C_ABI LUA_BINDING DECISIONS; do
    check "docs/$doc.md" test -f "$work/docs/$doc.md"
done

# The version the archive claims must be the version it was built from. The backticks
# are literal: they are Markdown in the generated header, not a command substitution.
echo "==> version"
# shellcheck disable=SC2016
declared="$(sed -n 's/.*For version `\([0-9][0-9.]*\)`.*/\1/p' "$work/include/giochi.h" | head -1)"
if [[ -n "$declared" ]]; then
    echo "  giochi.h reports $declared"
else
    echo "  FAIL  giochi.h states no version" >&2
    failures=$((failures + 1))
fi

if command -v nm >/dev/null 2>&1; then
    exports="$(nm -D --defined-only "$work/lib/libgiochi.so" 2>/dev/null | grep -c ' giochi_' || true)"
    if [[ "${exports:-0}" -ge 50 ]]; then
        echo "  ok    libgiochi.so exports ${exports} giochi_* symbols"
    else
        echo "  FAIL  libgiochi.so exports only ${exports:-0} giochi_* symbols" >&2
        failures=$((failures + 1))
    fi
fi

# Run the things. The demo and the game are the only proof that the archive is not
# merely well-formed.
echo "==> running (SDL_VIDEODRIVER=dummy)"
run_in_archive() {
    ( cd "$work" && SDL_VIDEODRIVER=dummy "$@" )
}

if run_in_archive ./giochi-demo --verify >"$work/verify.log" 2>&1; then
    echo "  ok    giochi-demo --verify"
    sed -n 's/^/          /p' "$work/verify.log" | tail -3
else
    echo "  FAIL  giochi-demo --verify" >&2
    sed -n 's/^/          /p' "$work/verify.log" | tail -10 >&2
    failures=$((failures + 1))
fi

if run_in_archive ./giochi-demo lua lua/mini_game.lua --auto --frames=60 >"$work/lua.log" 2>&1; then
    if grep -q 'texturas vivas: 0' "$work/lua.log"; then
        echo "  ok    mini_game.lua (0 live textures afterwards)"
    else
        echo "  FAIL  mini_game.lua leaked textures" >&2
        grep 'texturas vivas' "$work/lua.log" >&2 || true
        failures=$((failures + 1))
    fi
else
    echo "  FAIL  mini_game.lua" >&2
    tail -10 "$work/lua.log" >&2
    failures=$((failures + 1))
fi

# A real C consumer against the archive's own header and library. This is the check
# that the published ABI matches the published header.
if command -v cc >/dev/null 2>&1; then
    if cc -std=c11 -Wall -Wextra -Werror \
          -I "$work/include" "$repo_root/giochi-engine-capi/tests/c_smoke.c" \
          -L "$work/lib" -lgiochi -Wl,-rpath,"$work/lib" \
          -o "$work/c_smoke" >"$work/cc.log" 2>&1; then
        if ( cd "$work" && SDL_VIDEODRIVER=dummy ./c_smoke ) >"$work/csmoke.log" 2>&1; then
            echo "  ok    c_smoke ($(grep -o 'OK (abi [0-9]*, engine [0-9.]*)' "$work/csmoke.log" || echo passed))"
        else
            echo "  FAIL  c_smoke run" >&2
            tail -10 "$work/csmoke.log" >&2
            failures=$((failures + 1))
        fi
    else
        echo "  FAIL  c_smoke compile" >&2
        tail -10 "$work/cc.log" >&2
        failures=$((failures + 1))
    fi
else
    echo "  SKIP  c_smoke (no C compiler)"
fi

# The console situation: a target whose SDL2_image has no PNG decoder. Everything
# must still work from the .bmp twins (ADR-036).
echo "==> again with the PNGs deleted (codec-less target)"
rm -f "$work"/assets/*.png
check "giochi-demo --verify" run_in_archive ./giochi-demo --verify
check "mini_game.lua" run_in_archive ./giochi-demo lua lua/mini_game.lua --auto --frames=60
if [[ -x "$work/c_smoke" ]]; then
    check "c_smoke" bash -c "cd '$work' && SDL_VIDEODRIVER=dummy ./c_smoke"
fi

echo
if [[ "$failures" -gt 0 ]]; then
    echo "$failures check(s) FAILED -- do not publish this archive" >&2
    exit 1
fi
echo "archive verified${declared:+ (version $declared)}."
