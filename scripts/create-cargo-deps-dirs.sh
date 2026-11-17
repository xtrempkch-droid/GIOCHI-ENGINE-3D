#!/usr/bin/env bash
# Create the cargo `deps` directories before anything is built.
#
# `sdl2-sys`'s build script compiles SDL2 from source (the `bundled` feature, which
# every crate here enables for the core library) and then copies every library it
# produced into two places: the profile directory and that directory's `deps/`. It
# assumes `deps/` exists, and `fs::copy` into a missing directory fails with ENOENT,
# which the script reports as:
#
#   Failed to copy SDL2 dynamic library from .../out/lib/libSDL2_test.a
#                                        to .../release/deps/libSDL2_test.a
#
# The parent copy has just succeeded in that situation, so the destination in the
# message is the clue: it is always the `deps/` one. Cargo up to 1.99 created that
# directory eagerly; cargo 1.100 stopped doing it, so on a cold target directory the
# build dies before compiling a single crate of this workspace. It looks like a code
# bug and is not.
#
# Reproduced on cargo 1.100.0 by building this workspace into an empty
# CARGO_TARGET_DIR: exit 101 with the message above. Creating the directory and
# re-running the identical command: exit 0.
#
# Run this from `scripts/gates.sh`, from the cross-compile wrappers and from every CI
# job that builds. `CARGO_TARGET_DIR` is honoured, so a caller that redirects the
# target directory still gets the right paths.
#
# Removing this: when `cargo +stable build` succeeds in an empty target directory,
# delete the script, its callers, and the note in CONTRIBUTING.md (ADR-060).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"

triples=(
    x86_64-unknown-linux-gnu
    aarch64-unknown-linux-gnu
)
profiles=(debug release)

for profile in "${profiles[@]}"; do
    mkdir -p "$target_dir/$profile/deps"
    for triple in "${triples[@]}"; do
        mkdir -p "$target_dir/$triple/$profile/deps"
    done
done

echo "cargo deps dirs ready under $target_dir"
