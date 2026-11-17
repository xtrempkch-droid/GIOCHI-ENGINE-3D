#!/usr/bin/env bash
# Build the editor in release and lay out a folder that runs on its own.
#
# Usage:
#   scripts/package-editor.sh                 # into dist/
#   OUT=/tmp/pkg scripts/package-editor.sh    # somewhere else
#   SKIP_BUILD=1 scripts/package-editor.sh    # reuse the last release build
#
# Why a folder and not an archive: the editor needs its icons and its scenes next to the
# binary, and on GIOCHI-OS the "install" is copying a folder onto the SD card. An archive
# would make every bug report start with "did you extract it?".
#
# What goes in, and why:
#   giochi-editor          the binary
#   assets/icons/*.png     the hierarchy icons; without them the tree falls back to letters
#   assets/scenes/*.scene  a scene to open on the first run, so "it works" is visible
#   README.txt             how to run it, including `--config` for a portable install
#   USER_GUIDE.md          the manual
#   VERSION                the version, for a human and for a script
#
# What does not: the C sources, the tests, the engine's crates, `target/`. The editor does
# not link them at runtime — everything is in the binary.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

out_root="${OUT:-$repo_root/dist}"
target_triple="${TARGET:-x86_64-unknown-linux-gnu}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' giochi-editor/Cargo.toml | head -1)"
if [ -z "$version" ]; then
    echo "could not read the editor version from giochi-editor/Cargo.toml" >&2
    exit 1
fi

package="$out_root/giochi-editor-$version-$target_triple"

if [ "${SKIP_BUILD:-0}" != "1" ]; then
    echo "==> cargo build --release -p giochi-editor"
    cargo build --release -p giochi-editor --target "$target_triple"
fi

binary="target/$target_triple/release/giochi-editor"
if [ ! -x "$binary" ]; then
    echo "missing $binary; run without SKIP_BUILD=1 to build it" >&2
    exit 1
fi

echo "==> assembling $package"
rm -rf "$package"
mkdir -p "$package/assets/icons" "$package/assets/scenes"
install -m 755 "$binary" "$package/giochi-editor"
cp -a giochi-editor/assets/icons/. "$package/assets/icons/"
cp -a giochi-editor/assets/scenes/. "$package/assets/scenes/"
install -m 644 docs/EDITOR_USER_GUIDE.md "$package/USER_GUIDE.md"
printf '%s\n' "$version" > "$package/VERSION"

cat > "$package/README.txt" <<'TXT'
giochi-editor — visual scene editor for GIOCHI-ENGINE
=====================================================

Run it:

    ./giochi-editor

Open a scene, or start with the one that ships:

    ./giochi-editor --scene assets/scenes/default.scene

A portable install (settings and autosaves inside this folder, not in your home):

    ./giochi-editor --config ./config

Headless check (renders a few frames and exits, for a CI or an SD card test):

    ./giochi-editor --frames 30

Keys:

    Q W E R     select / move / rotate / scale
    Ctrl+Z      undo            Ctrl+Y  redo
    Ctrl+S      save            F5      play preview
    F2          console         Esc     quit

The manual is USER_GUIDE.md.

Requirements: a desktop driver (X11/Wayland) for the editor itself; the play preview needs a
video driver that can open a second window and provide GLES 2.0. On KMSDRM the preview will
say so in the status bar and the editor keeps working.

The icons and the default scene are required: the editor looks for `assets/` next to the
binary (or next to the current directory) and falls back to drawing text labels without them.
TXT

# The manifest is what `scripts/verify-editor-package.sh` and a human both read. Written to a
# temporary file and moved into place: hashing into a file the `find` is walking is how a
# manifest ends up listing itself.
# The list is written *outside* the folder being walked: a temporary file inside it shows up
# in its own manifest, which then fails verification.
manifest_tmp="$(mktemp)"
(
    cd "$package"
    find . -type f -print0 | sort -z | xargs -0 sha256sum > "$manifest_tmp"
)
mv "$manifest_tmp" "$package/MANIFEST.txt"

echo "==> manifest"
cat "$package/MANIFEST.txt"
echo
echo "packaged $package"
