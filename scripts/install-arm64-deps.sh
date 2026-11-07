#!/usr/bin/env bash
# Install the ARM64 build dependencies for cross-compiling the workspace.
#
# Two things are needed that `bundled` does not provide:
#
#   1. The ARM64 SDL2 satellites. `bundled` builds *core* SDL2 from source, but the
#      `image`, `mixer` and `ttf` features link the **system** SDL2_image,
#      SDL2_mixer and SDL2_ttf. For a cross build those must exist for aarch64.
#   2. The ARM64 cross toolchain (`aarch64-linux-gnu-gcc`).
#
# The recipe is Debian/Ubuntu multiarch: `dpkg --add-architecture arm64` plus the
# ports.ubuntu.com mirror, because archive.ubuntu.com carries amd64 only.
#
# Two details that are easy to get wrong, and both cost a red CI run:
#
#   * The existing (amd64) sources must be pinned with `Architectures: amd64`
#     first. Without it apt tries to fetch arm64 from the amd64 mirror and 404s.
#   * The ports suite must be the **runner's own** codename. Hard-coding `noble` on
#     a newer runner mixes indexes from two releases, and the libc packages then
#     conflict: `libcrypt-dev:amd64 Breaks libcrypt-dev:arm64 (!= <newer>)`.
#
# Usage:
#   sudo scripts/install-arm64-deps.sh
set -euo pipefail

if [[ "$(id -u)" != "0" ]]; then
    echo "error: run as root (sudo scripts/install-arm64-deps.sh)" >&2
    exit 1
fi

# The runner's own release, so both architectures come from the same index.
# shellcheck disable=SC1091 # a runtime file; shellcheck cannot follow it without -x
. /etc/os-release
codename="${VERSION_CODENAME:-}"
if [[ -z "$codename" ]]; then
    echo "error: /etc/os-release has no VERSION_CODENAME; cannot pick the ports suite" >&2
    exit 1
fi

echo "==> release: ${PRETTY_NAME:-unknown} (codename '$codename')"

# 1. Pin the amd64 sources. `ubuntu.sources` is the deb822 file on 24.04+;
#    older images still use the one-line `sources.list` format.
changed_amd64=0
for file in /etc/apt/sources.list.d/ubuntu.sources /etc/apt/sources.list.d/debian.sources; do
    [[ -f "$file" ]] || continue
    if ! grep -q '^Architectures:' "$file"; then
        sed -i 's/^Types: deb$/Types: deb\nArchitectures: amd64/' "$file"
        changed_amd64=1
    fi
done
if [[ "$changed_amd64" == "0" ]] && ! grep -rqs '^Architectures: amd64' /etc/apt/sources.list.d/; then
    echo "==> warning: no deb822 sources found to pin to amd64; assuming they already are"
fi

# 2. Add the ARM64 ports mirror for the same release.
dpkg --add-architecture arm64
cat > /etc/apt/sources.list.d/arm64-ports.sources <<EOF
Types: deb
URIs: http://ports.ubuntu.com/ubuntu-ports
Suites: ${codename} ${codename}-updates ${codename}-security
Components: main universe
Architectures: arm64
EOF

apt-get update -qq

# 3. Install. `build-essential`/`cmake`/`pkg-config` are only needed on a bare
#    image; on a runner they are already there and apt says so.
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends \
    build-essential cmake pkg-config file \
    gcc-aarch64-linux-gnu g++-aarch64-linux-gnu \
    libsdl2-image-dev:arm64 libsdl2-mixer-dev:arm64 libsdl2-ttf-dev:arm64

echo "==> arm64 SDL2 satellites:"
ls -1 /usr/lib/aarch64-linux-gnu/libSDL2_* 2>/dev/null || {
    echo "error: the arm64 SDL2 satellites are still missing" >&2
    exit 1
}
echo "==> done"
