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
force_low_battery=0

usage() {
    cat <<'USAGE'
Validate the engine on a real console over SSH, then hand the screen back.

  --host USER@HOST   SSH destination (required; key authentication expected)
  --sysroot DIR      ARM64 sysroot for the cross build and the C consumer
  --dest DIR         deployment directory on the device (default /opt/giochi)
  --frames N         frames per run (default 300)
  --skip-build       reuse an existing target/aarch64-unknown-linux-gnu/release
  --skip-c-smoke     do not cross-compile and run the C consumer
  --force-low-battery  run even when the battery cannot carry the session
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
        --force-low-battery) force_low_battery=1; shift ;;
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

# A brown-out on this class of device does not just end the run: the rootfs does
# not survive one, and the deploy plus the SSH key go with it (docs/KMSDRM.md
# section 8.10). Running GLES at 0 mAh is therefore a destructive act, not a
# risky one, so the battery is checked before anything is written to the flash.
echo
echo "==> checking the battery"
battery=$(remote <<'REMOTE' || true
for d in /sys/class/power_supply/*/; do
    [ "$(cat "$d/type" 2>/dev/null)" = "Battery" ] || continue
    capacity=$(cat "$d/capacity" 2>/dev/null)
    status=$(cat "$d/status" 2>/dev/null)
    voltage=$(cat "$d/voltage_avg" 2>/dev/null)
    minimum=$(cat "$d/voltage_min_design" 2>/dev/null)
    printf '%s %s %s %s' "${capacity:-?}" "${status:-?}" "${voltage:-?}" "${minimum:-?}"
    break
done
REMOTE
)
read -r batt_capacity batt_status batt_voltage batt_minimum <<<"$battery"

charging=$(remote <<'REMOTE' || true
for d in /sys/class/power_supply/*/; do
    case "$(cat "$d/type" 2>/dev/null)" in
        Mains|USB|USB_C) [ "$(cat "$d/online" 2>/dev/null)" = "1" ] && echo yes && exit 0;;
    esac
done
echo no
REMOTE
)

if [[ -n "${batt_capacity:-}" && "$batt_capacity" != "?" ]]; then
    echo "  battery: ${batt_capacity}% ${batt_status}, ${batt_voltage} uV (design minimum ${batt_minimum} uV), charging: $charging"
    # `<= 5% and not charging` is the condition that was measured to matter. The
    # voltage check catches a pack that still reports a percentage while already
    # below its design minimum, which is what a worn cell does.
    below_minimum=0
    if [[ "$batt_voltage" =~ ^[0-9]+$ && "$batt_minimum" =~ ^[0-9]+$ ]]; then
        [[ "$batt_voltage" -lt "$batt_minimum" ]] && below_minimum=1
    fi
    if [[ "$charging" != "yes" ]] && { [[ "$batt_capacity" -le 5 ]] || [[ "$below_minimum" == "1" ]]; }; then
        if [[ "$force_low_battery" == "0" ]]; then
            die "the battery cannot carry this run. Charge the device, then re-run.
       Running GLES now risks a brown-out, and a brown-out wipes the rootfs --
       which is where the deploy and the SSH key live. Use --force-low-battery to
       override."
        fi
        echo "  WARNING: continuing because --force-low-battery was given"
    fi
else
    echo "  no battery reported; skipping the check"
fi

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
        remote "$launcher" <<'REMOTE' || echo "  warning: restore did not confirm. On the device run: chmod +x $launcher; kill -TERM \$(ps | grep '[g]iochi-init' | awk '{print \$1}')" >&2
launcher="$1"

# Returns 0 and prints "<pid> <comm>" for the process holding card0, 1 if nobody is.
drm_owner() {
    for p in /proc/[0-9]*; do
        for f in "$p"/fd/*; do
            case "$(readlink "$f" 2>/dev/null)" in
                /dev/dri/card0) echo "${p#/proc/} $(cat "$p/comm" 2>/dev/null)"; return 0;;
            esac
        done
    done 2>/dev/null
    return 1
}

if [ -e "$launcher" ]; then
    chmod 0755 "$launcher" || true
fi
rm -f /run/giochi/launcher-parado

# The supervisor re-checks `-x` on every pass, so re-enabling the file is what
# brings the console back; its own poll does it within $LAUNCHER_WAIT (30 s on this
# image). Waiting is preferred to signalling: /etc/inittab registers the
# supervisor as `::respawn:`, so a kill only makes BusyBox start a new one, and
# that is a heavier hammer than the job needs.
owner=""
i=0
while [ $i -lt 35 ]; do
    owner=$(drm_owner) || owner=""
    if [ -n "$owner" ]; then break; fi
    i=$((i + 1))
    sleep 1
done

# Only if the natural poll has not acted -- a supervisor that was parked in
# `sleep 3600` after giving up needs the nudge to re-enter its loop.
if [ -z "$owner" ]; then
    for pid in $(ps | grep '[g]iochi-init' | awk '{print $1}'); do
        kill -TERM "$pid" 2>/dev/null || true
    done
    i=0
    while [ $i -lt 35 ]; do
        owner=$(drm_owner) || owner=""
        if [ -n "$owner" ]; then break; fi
        i=$((i + 1))
        sleep 1
    done
fi

launchers=$(ps | grep -c '[g]iochi-launcher' 2>/dev/null || true)
echo "  launcher processes: ${launchers:-0}"
echo "  DRM master: ${owner:-nobody}"
if [ -e /run/giochi/launcher-parado ]; then
    echo "  marker present: /run/giochi/launcher-parado"
fi

# Positive confirmation: the console is only really restored when something holds
# card0 again. Ending on a bare `[ ... ] && echo` reports a false failure whenever
# the test is false -- which is how the first version of this script lied.
if [ -n "$owner" ]; then
    echo "  console restored"
    exit 0
fi
echo "  the console did NOT come back" >&2
exit 1
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
mkdir -p "$dest/assets" "$dest/lib" "$dest/lua" "$dest/bin"
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

# The 3D examples are separate binaries, not subcommands of giochi-demo: they
# need a GL context and the 2D demo deliberately never asks for one.
for example in cube_demo mini_fps ball_balance; do
    [[ -x "$target_dir/examples/$example" ]] ||
        die "$target_dir/examples/$example is missing; build with 'scripts/build-aarch64.sh --release --examples'"
    scp -q -o BatchMode=yes "$target_dir/examples/$example" "$host:$dest/bin/$example"
done

echo "==> releasing the console (the launcher holds DRM master)"
console_released=1
remote "$launcher" <<'REMOTE'
launcher="$1"

drm_holders() {
    for p in /proc/[0-9]*; do
        for f in "$p"/fd/*; do
            case "$(readlink "$f" 2>/dev/null)" in
                /dev/dri/*) echo "${p#/proc/}";;
            esac
        done
    done 2>/dev/null | sort -u
}

# Order matters, and both steps were measured the hard way on a PX30:
#
# 1. Make the launcher non-executable FIRST. The supervisor re-checks `-x` on
#    every pass and parks in `sleep $LAUNCHER_WAIT` when it is false, so nothing
#    respawns a new DRM master behind us.
# 2. Never kill the supervisor. /etc/inittab registers it as
#    `::respawn:/usr/bin/giochi-init`, so killing it only makes BusyBox start a
#    fresh one -- and a supervisor that was blocked in `"$LAUNCHER"` left an
#    orphaned launcher child behind, which kept DRM master. Measured: that is
#    exactly the loop that made the first version of this script hang.
#
# Killing the launcher once is enough. The supervisor classifies status 143
# (SIGTERM) as a deliberate exit, zeroes its crash quota and goes to wait for the
# launcher to reappear -- no /run/giochi/launcher-parado marker, no give-up.
#
# `chmod 0644`, not `chmod -x`: BusyBox applies the umask to a symbolic mode, and
# this image runs with umask 0077, so `chmod -x` left `-rw-r-xr-x` -- still
# executable, and since everything here runs as root `[ -x ]` stayed true. The
# supervisor then respawned the launcher mid-run and DRM never came free.
if [ -e "$launcher" ]; then
    chmod 0644 "$launcher" || true
    if [ -x "$launcher" ]; then
        echo "  could not clear the execute bit on $launcher" >&2
        exit 1
    fi
fi

kill_holders() {
    signal="$1"
    for pid in $(drm_holders); do
        comm=$(cat "/proc/$pid/comm" 2>/dev/null)
        case "$comm" in
            giochi-launcher|giochi-demo)
                kill "-$signal" "$pid" 2>/dev/null || true
                ;;
        esac
    done
}

# The launcher installs a SIGTERM handler, so TERM usually works but is not
# guaranteed -- measured both ways on the same unit, depending on what it was
# blocked in. Give it a grace period, then escalate; a KILL also only counts as
# one of the supervisor's five crashes before it parks.
i=0
while [ $i -lt 10 ]; do
    [ -z "$(drm_holders)" ] && break
    kill_holders TERM
    i=$((i + 1))
    sleep 1
done

i=0
while [ $i -lt 10 ]; do
    [ -z "$(drm_holders)" ] && break
    kill_holders KILL
    i=$((i + 1))
    sleep 1
done

holders=$(drm_holders)
if [ -n "$holders" ]; then
    echo "  DRM still held by:" >&2
    for pid in $holders; do
        echo "    $pid $(cat /proc/$pid/comm 2>/dev/null)" >&2
    done
    echo "  Kill it from another shell, then re-run with --skip-build:" >&2
    echo "    ssh <host> 'kill -KILL <pid>'" >&2
    exit 1
fi
echo "  console free (DRM master released)"
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

report "3D cube_demo --frames $frames"
if remote "$dest" "$frames" <<'REMOTE'
cd "$1/bin" || exit 1
./cube_demo --frames "$2" 2>&1
REMOTE
then :; else failures=$((failures + 1)); fi

report "3D mini_fps --frames $frames --auto"
if remote "$dest" "$frames" <<'REMOTE'
cd "$1/bin" || exit 1
./mini_fps --frames "$2" --auto 2>&1
REMOTE
then :; else failures=$((failures + 1)); fi

# The balance game is the latency check a human can actually make, so it is run
# often enough to be trusted. `--auto` because nobody is holding the stick here;
# what is asserted is that the ball ended up where the script pushed it, which is
# the same claim the unit tests make about the physics and this one makes about
# the whole binary.
report "3D ball_balance --frames $frames --auto"
balance_log="$work/balance.log"
if remote "$dest" "$frames" <<'REMOTE' >"$balance_log" 2>&1
cd "$1/bin" || exit 1
./ball_balance --frames "$2" --auto 2>&1
REMOTE
then
    sed -e 's/^/  /' -e 's/^  /  /' "$balance_log"
    ball_x=$(sed -n 's/.*bola em (\([+-][0-9.]*\),.*/\1/p' "$balance_log" | tail -1)
    if grep -q "no GL error" "$balance_log"; then :; else
        echo "  FAIL ball_balance reported a GL error" >&2
        failures=$((failures + 1))
    fi
    if [[ -n "$ball_x" ]] && awk -v x="$ball_x" 'BEGIN { exit !(x > 0.5) }'; then :; else
        echo "  FAIL the script pushes right; the ball ended at x='${ball_x:-missing}'" >&2
        failures=$((failures + 1))
    fi
    # The reported work must be the game's own cost, not the vsync wait. KMSDRM's
    # `end_frame` blocks until the panel refreshes, so a clock read after the swap
    # charges every frame a full refresh interval -- the indicator then sits on its own
    # threshold and reports the display rather than a stutter. Anything near 16.7 ms
    # here means exactly that.
    ball_p99=$(sed -n 's/.*p99 \([0-9][0-9.]*\).*/\1/p' "$balance_log" | tail -1)
    if [[ -n "$ball_p99" ]] && awk -v v="$ball_p99" 'BEGIN { exit !(v < 10.0) }'; then :; else
        echo "  FAIL frame work p99 is '${ball_p99:-missing}' ms; a value near a refresh" >&2
        echo "       interval (16.7 ms at 60 Hz) means the clock is read after the swap" >&2
        failures=$((failures + 1))
    fi
else
    sed -e 's/^/  /' "$balance_log" >&2
    failures=$((failures + 1))
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
