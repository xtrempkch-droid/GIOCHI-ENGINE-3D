#!/usr/bin/env bash
# Validate the engine on a real console over SSH, then hand the screen back.
#
# Everything this script does was learned the hard way on a Rockchip PX30 handheld
# (docs/KMSDRM.md section 8). It exists so the next unit does not need that
# archaeology repeated:
#
#   * the console's own `giochi-launcher` holds DRM master for the whole session,
#     so "another process holds DRM master" does not mean "stop your X server";
#   * after five crashes in a minute the supervisor gives up permanently and parks
#     in `sleep 3600`, leaving a black screen and a marker at
#     /run/giochi/launcher-parado — recovery is signalling `giochi-init`, not
#     rebooting;
#   * a freed console still has an orphaned launcher child holding /dev/dri.
#
# The screen is released before the checks run and restored afterwards, including
# on Ctrl-C and on failure. The device is left exactly as it was found.
#
# Usage:
#   scripts/hardware-check.sh --host root@10.55.0.2 --sysroot /tmp/r35s-sysroot
#   scripts/hardware-check.sh --host root@10.55.0.2 --skip-build --frames 600
#
# Options:
#   --host USER@HOST   SSH destination (required; key authentication expected)
#   --sysroot DIR      ARM64 sysroot for the cross build and the C consumer
#   --dest DIR         deployment directory on the device (default /opt/giochi)
#   --frames N         frames per run (default 300)
#   --skip-build       reuse an existing target/aarch64-unknown-linux-gnu/release
#   --skip-c-smoke     do not cross-compile and run the C consumer
#   -h, --help         this text
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

target=aarch64-unknown-linux-gnu
host=""
sysroot="${CROSS_SYSROOT:-}"
dest="/opt/giochi"
frames=300
skip_build=0
skip_c_smoke=0

usage() {
    cat <<'USAGE'
Validate the engine on a real console over SSH, then hand the screen back.

  --host USER@HOST   SSH destination (required; key authentication expected)
  --sysroot DIR      ARM64 sysroot for the cross build and the C consumer
  --dest DIR         deployment directory on the device (default /opt/giochi)
  --frames N         frames per run (default 300)
  --skip-build       reuse an existing target/aarch64-unknown-linux-gnu/release
  --skip-c-smoke     do not cross-compile and run the C consumer
  -h, --help         this text
USAGE
}

die() {
    echo "error: $*" >&2
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --host) host="${2:-}"; shift 2 ;;
        --sysroot) sysroot="${2:-}"; shift 2 ;;
        --dest) dest="${2:-}"; shift 2 ;;
        --frames) frames="${2:-}"; shift 2 ;;
        --skip-build) skip_build=1; shift ;;
        --skip-c-smoke) skip_c_smoke=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) die "unknown option: $1 (try --help)" ;;
    esac
done

[[ -n "$host" ]] || { usage; echo; die "--host is required"; }
[[ "$frames" =~ ^[0-9]+$ ]] || die "--frames must be a number, got '$frames'"

target_dir="${CARGO_TARGET_DIR:-$repo_root/target}/$target/release"
launcher="$dest/apps/launcher/bin/giochi-launcher"

# `sh -s -- ARGS` runs a POSIX sh snippet on the device. The device is Buildroot:
# it has busybox `sh`, `awk`, `ps` and `readlink`, but no bash, no python3, no
# timeout and no fuser. Every remote snippet below sticks to that set.
remote() {
    ssh -o BatchMode=yes -o ConnectTimeout=15 "$host" sh -s -- "$@"
}

# One liveness probe before anything else: a stale address produces a confusing
# build-then-fail sequence otherwise.
echo "==> probing $host"
remote <<'REMOTE' || die "$host is not answering over SSH"
echo "  host:    $(cat /etc/hostname 2>/dev/null || echo '?')"
echo "  kernel:  $(uname -r) $(uname -m)"
echo "  drm:     $(ls /dev/dri 2>/dev/null | tr '\n' ' ')"
REMOTE

if [[ "$skip_build" == "0" ]]; then
    echo
    echo "==> cross-building for $target (release)"
    if [[ -n "$sysroot" ]]; then
        CROSS_SYSROOT="$sysroot" scripts/build-aarch64.sh --release
    else
        scripts/build-aarch64.sh --release
    fi
else
    [[ -f "$target_dir/giochi-demo" ]] ||
        die "--skip-build given but $target_dir/giochi-demo does not exist"
    echo "==> reusing $target_dir"
fi

work="$(mktemp -d)"
restored=0
console_released=0

restore_console() {
    local status=$?
    if [[ "$console_released" == "1" && "$restored" == "0" ]]; then
        restored=1
        echo
        echo "==> restoring the console"
        remote "$launcher" <<'REMOTE' || echo "  warning: restore failed. On the device: chmod +x $launcher && rm -f /run/giochi/launcher-parado && kill -TERM \$(ps | grep '[g]iochi-init' | awk '{print \$1}')" >&2
launcher="$1"
[ -e "$launcher" ] && chmod +x "$launcher"
rm -f /run/giochi/launcher-parado
for pid in $(ps | grep '[g]iochi-init' | awk '{print $1}'); do
    kill -TERM "$pid" 2>/dev/null || true
done
i=0
while [ $i -lt 20 ]; do
    running=$(ps | grep -c '[g]iochi-launcher' 2>/dev/null || true)
    [ "${running:-0}" -gt 0 ] && break
    i=$((i + 1))
    sleep 1
done
running=$(ps | grep -c '[g]iochi-launcher' 2>/dev/null || true)
echo "  launcher processes: ${running:-0}"
[ -e /run/giochi/launcher-parado ] && echo "  marker still present: /run/giochi/launcher-parado"
REMOTE
    fi
    rm -rf "$work"
    exit "$status"
}
trap restore_console EXIT INT TERM

c_smoke="$work/c_smoke"
if [[ "$skip_c_smoke" == "0" ]]; then
    command -v aarch64-linux-gnu-gcc >/dev/null 2>&1 ||
        die "aarch64-linux-gnu-gcc not found; pass --skip-c-smoke to run without the C consumer"
    [[ -f "$target_dir/libgiochi.so" ]] ||
        die "$target_dir/libgiochi.so missing; the C consumer needs the shared library"

    echo
    echo "==> cross-compiling the C consumer"
    # The sysroot usually has libSDL2*.so but no libc headers, so the *consumer*
    # compiles against the host's cross headers and only links against the sysroot
    # for the sonames. `--allow-shlib-undefined` is required because libgiochi.so
    # depends on SDL2 libraries that live on the device, not on this machine.
    # shellcheck disable=SC2054 # the commas are part of the option, not a separator
    link_flags=(-Wl,--allow-shlib-undefined)
    if [[ -n "$sysroot" ]]; then
        # shellcheck disable=SC2054
        link_flags+=(-Wl,-rpath-link="$sysroot/usr/lib")
    fi
    aarch64-linux-gnu-gcc -std=c11 -Wall -Wextra -Werror \
        -I giochi-engine-capi/include \
        giochi-engine-capi/tests/c_smoke.c \
        -L "$target_dir" -lgiochi \
        "${link_flags[@]}" \
        -o "$c_smoke"
    file "$c_smoke" | grep -q 'ARM aarch64' ||
        die "$c_smoke is not an AArch64 executable"
fi

echo
echo "==> deploying to $host:$dest"
remote "$dest" <<'REMOTE'
set -e
dest="$1"
mkdir -p "$dest/assets" "$dest/lib" "$dest/lua"
printf '%s' "$dest"
REMOTE

scp -q -o BatchMode=yes \
    "$target_dir/giochi-demo" "$host:$dest/giochi-demo"
scp -q -o BatchMode=yes -r assets/. "$host:$dest/assets/"
scp -q -o BatchMode=yes \
    giochi-engine-lua/examples/mini_game.lua "$host:$dest/lua/mini_game.lua"
scp -q -o BatchMode=yes \
    "$target_dir/libgiochi.so" "$host:$dest/lib/libgiochi.so"
if [[ "$skip_c_smoke" == "0" ]]; then
    scp -q -o BatchMode=yes "$c_smoke" "$host:$dest/lib/c_smoke"
fi

echo "==> releasing the console (the launcher holds DRM master)"
console_released=1
remote "$launcher" "$dest" <<'REMOTE'
launcher="$1"
dest="$2"

drm_holders() {
    for p in /proc/[0-9]*; do
        for f in "$p"/fd/*; do
            case "$(readlink "$f" 2>/dev/null)" in
                /dev/dri/*) echo "${p#/proc/}";;
            esac
        done
    done 2>/dev/null | sort -u
}

# The supervisor only respawns an executable launcher into the *next* loop
# iteration; making it non-executable parks it in `sleep $LAUNCHER_WAIT` instead
# of respawning a process that would immediately grab DRM again.
if [ -e "$launcher" ]; then
    chmod -x "$launcher" || true
fi

for pid in $(ps | grep '[g]iochi-init' | awk '{print $1}'); do
    kill -TERM "$pid" 2>/dev/null || true
done

i=0
while [ $i -lt 30 ]; do
    holders=$(drm_holders)
    [ -z "$holders" ] && break
    for pid in $holders; do
        comm=$(cat "/proc/$pid/comm" 2>/dev/null)
        case "$comm" in
            giochi-launcher|giochi-init|giochi-demo) kill -TERM "$pid" 2>/dev/null || true;;
        esac
    done
    i=$((i + 1))
    sleep 1
done

holders=$(drm_holders)
if [ -n "$holders" ]; then
    echo "  DRM still held by:" >&2
    for pid in $holders; do
        echo "    $pid $(cat /proc/$pid/comm 2>/dev/null)" >&2
    done
    exit 1
fi
echo "  console free after ${i}s"
REMOTE

echo
echo "==> running the checks on the device"
failures=0
report() {
    echo
    echo "--- $1"
}

# `--verify` prints the live texture count, so "the texture loaded" is asserted
# rather than assumed from the absence of an error.
report "demo --verify"
if remote "$dest" <<'REMOTE'
cd "$1" || exit 1
./giochi-demo --verify 2>&1
REMOTE
then :; else failures=$((failures + 1)); fi

report "demo --frames $frames --quadro"
if remote "$dest" "$frames" <<'REMOTE'
cd "$1" || exit 1
./giochi-demo --frames "$2" --quadro /tmp/giochi-frame.bmp 2>&1
REMOTE
then :; else failures=$((failures + 1)); fi

# "Opened the window" and "drew something" are different claims, and over SSH the
# only way to tell them apart is to bring the pixels back.
frame="$work/frame.bmp"
if scp -q -o BatchMode=yes "$host:/tmp/giochi-frame.bmp" "$frame" 2>/dev/null; then
    read -r frame_w frame_h < <(
        od -An -tu4 -j18 -N8 "$frame" | awk '{print $1, $2}'
    )
    echo "  captured frame: ${frame_w}x${frame_h}, $(stat -c %s "$frame") bytes"
    if [[ "${frame_w:-0}" -le 0 || "${frame_h:-0}" -le 0 ]]; then
        echo "  FAIL the captured frame has no dimensions" >&2
        failures=$((failures + 1))
    fi
else
    echo "  FAIL no frame was captured" >&2
    failures=$((failures + 1))
fi

report "lua mini_game --frames $frames"
if remote "$dest" "$frames" <<'REMOTE'
cd "$1" || exit 1
./giochi-demo lua lua/mini_game.lua --auto --frames "$2" 2>&1
REMOTE
then :; else failures=$((failures + 1)); fi

if [[ "$skip_c_smoke" == "0" ]]; then
    report "c_smoke"
    if remote "$dest" <<'REMOTE'
cd "$1" || exit 1
LD_LIBRARY_PATH="$1/lib:${LD_LIBRARY_PATH:-}" ./lib/c_smoke 2>&1
REMOTE
    then :; else failures=$((failures + 1)); fi
fi

report "resources"
remote <<'REMOTE' || true
echo "  battery: $(cat /sys/class/power_supply/*/capacity 2>/dev/null | head -1)% ($(cat /sys/class/power_supply/*/status 2>/dev/null | head -1))"
REMOTE

echo
if [[ "$failures" -gt 0 ]]; then
    echo "$failures check(s) FAILED on $host" >&2
else
    echo "all hardware checks passed on $host"
fi

# An explicit non-zero exit still runs the trap, which restores the console.
[[ "$failures" -eq 0 ]]
