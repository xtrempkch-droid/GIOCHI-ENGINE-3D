#!/usr/bin/env bash
# Check that a packaged editor is complete and actually runs.
#
# Usage:
#   scripts/verify-editor-package.sh dist/giochi-editor-0.1.0-x86_64-unknown-linux-gnu
#   scripts/verify-editor-package.sh            # the newest folder under dist/
#
# The point is to catch the two ways a package goes wrong and neither build nor tests can:
# a file left out of the folder, and a binary that only works from the repository root
# because it was finding `assets/` in the checkout.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

package="${1:-}"
if [ -z "$package" ]; then
    package="$(find dist -maxdepth 1 -type d -name 'giochi-editor-*' 2>/dev/null | sort | tail -1)"
fi
if [ -z "$package" ] || [ ! -d "$package" ]; then
    echo "no package to verify; run scripts/package-editor.sh first" >&2
    exit 1
fi

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

for required in giochi-editor README.txt USER_GUIDE.md VERSION MANIFEST.txt \
                assets/scenes/default.scene assets/icons/mesh_16.png; do
    [ -e "$package/$required" ] || fail "missing $package/$required"
done
[ -x "$package/giochi-editor" ] || fail "$package/giochi-editor is not executable"

# The manifest has to describe the folder it ships with.
echo "==> manifest"
(cd "$package" && sha256sum --quiet --check MANIFEST.txt)

# Run it from *inside* the package: a binary that only works from the repository root is
# the failure this whole script exists for.
echo "==> version"
(cd "$package" && ./giochi-editor --version)

echo "==> icons"
# A packaged editor that cannot find `assets/icons` still starts and still draws the tree —
# with text labels and one warning per icon. Looking for the warning is what makes "the
# icons travel with the binary" a checked property instead of a hope.
warnings="$(
    cd "$package"
    RUST_LOG=warn SDL_VIDEODRIVER=dummy SDL_RENDER_DRIVER=software \
        ./giochi-editor --frames 1 --config "$(mktemp -d)" 2>&1 | grep -c 'not loaded' || true
)"
[ "$warnings" = "0" ] || fail "$warnings icon(s) did not load from the package"

echo "==> headless run"
(
    cd "$package"
    config="$(mktemp -d)"
    SDL_VIDEODRIVER=dummy SDL_RENDER_DRIVER=software \
        ./giochi-editor --frames 3 --config "$config" \
        --scene assets/scenes/default.scene
    rm -rf "$config"
)

echo "package ok: $package"
