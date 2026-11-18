#!/usr/bin/env bash
# Deploy the racer to a console over SSH, run it on the real display, and hand the
# screen back.
#
# The whole run — release, execute, restore — is **one** remote script with its own
# `trap`, so a dropped SSH connection cannot leave the console black: the trap fires
# on the device, not on this side of the link.
#
# Everything about releasing DRM master is lifted from `scripts/hardware-check.sh`,
# which learned it the hard way on this hardware (docs/KMSDRM.md §8):
#
#   * `giochi-launcher` holds /dev/dri/card0 for the whole session, so the game cannot
#     get DRM master until it is gone;
#   * the launcher must be made **non-executable first**, or the supervisor
#     (`giochi-init`, registered in /etc/inittab as `::respawn:`) starts a new one
#     behind us mid-run — and `chmod 0644`, not `chmod -x`, because this image runs
#     with umask 0077 and BusyBox applies the umask to symbolic modes;
#   * never kill the supervisor while releasing: it only makes BusyBox respawn one and
#     leaves an orphaned launcher child holding DRM;
#   * restoring means re-enabling the launcher and waiting for the supervisor's own
#     poll to bring the UI back, nudging `giochi-init` only if 35 s go by.
#
# Usage:
#   giochi-racer/scripts/device-run.sh                      # 600 frames, scripted driver
#   giochi-racer/scripts/device-run.sh --frames 120
#   giochi-racer/scripts/device-run.sh --skip-deploy        # binary already on the device
#   giochi-racer/scripts/device-run.sh --binary path/to/giochi-racer
#   giochi-racer/scripts/device-run.sh --keyboard           # no --auto: reads the pad
#   giochi-racer/scripts/device-run.sh --size 320x240       # window size passed to the game
#   giochi-racer/scripts/device-run.sh --laps 1
#   giochi-racer/scripts/device-run.sh --keyboard --log debug   # pad-driven, verbose
#
# The frame is copied back to /tmp/racer-frame.bmp and checked with check-frame.py.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

host="root@10.55.0.2"
dest="/opt/racer"
frames=600
size=""
laps=""
log_level=""
binary="target/aarch64-unknown-linux-gnu/release/giochi-racer"
skip_deploy=0
auto=1
launcher="/opt/giochi/apps/launcher/bin/giochi-launcher"
local_frame="/tmp/racer-frame.bmp"

usage() {
    sed -n '2,30p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --host) host="$2"; shift 2;;
        --dest) dest="$2"; shift 2;;
        --frames) frames="$2"; shift 2;;
        --size) size="$2"; shift 2;;
        --laps) laps="$2"; shift 2;;
        --log) log_level="$2"; shift 2;;
        --binary) binary="$2"; shift 2;;
        --launcher) launcher="$2"; shift 2;;
        --skip-deploy) skip_deploy=1; shift;;
        --keyboard) auto=0; shift;;
        -h|--help) usage; exit 0;;
        *) echo "unknown argument '$1'; --help" >&2; exit 2;;
    esac
done

# ---------------------------------------------------------------------------
# Deploy
# ---------------------------------------------------------------------------
if [[ "$skip_deploy" == "0" ]]; then
    [[ -f "$binary" ]] || {
        echo "error: $binary does not exist; cross-build it first (see" \
             "docs/CROSS_COMPILE.md and giochi-racer/README.md)" >&2
        exit 1
    }
    file "$binary" | grep -q 'ARM aarch64' || {
        echo "error: $binary is not an AArch64 executable" >&2
        exit 1
    }
    echo "==> deploying to $host:$dest"
    ssh -o BatchMode=yes "$host" "mkdir -p '$dest/assets'"
    scp -q -o BatchMode=yes "$binary" "$host:$dest/giochi-racer"
    scp -q -o BatchMode=yes -r giochi-racer/assets/. "$host:$dest/assets/"
fi

# ---------------------------------------------------------------------------
# The device side. One script, one trap, both the release and the restore.
# ---------------------------------------------------------------------------
args="$frames $auto $dest $launcher ${size:--} ${laps:--} ${log_level:-info}"
# shellcheck disable=SC2086
ssh -o BatchMode=yes "$host" bash -s $args <<'REMOTE'
set -e
frames="$1"
auto="$2"
dest="$3"
launcher="$4"
size="$5"
laps="$6"
log_level="$7"
extra=""
[ "$size" != "-" ] && extra="$extra --size $size"
[ "$laps" != "-" ] && extra="$extra --laps $laps"

echo "==> device: $(uname -m) $(uname -r), $(nproc) cores, $(awk '/MemTotal/{printf "%d MB", $2/1024}' /proc/meminfo)"

# Every process holding a /dev/dri node. `card0` is the one that matters for DRM
# master; renderD128 is holding the render node open and does not block a KMS surface
# but is reported, because a launcher that kept it would be a leak.
drm_holders() {
    for p in /proc/[0-9]*; do
        for f in "$p"/fd/*; do
            case "$(readlink "$f" 2>/dev/null)" in
                /dev/dri/card0) echo "${p#/proc/}";;
            esac
        done
    done 2>/dev/null | sort -u
}

released=0
restored=0
restore() {
    status=$?
    if [ "$released" = "1" ] && [ "$restored" = "0" ]; then
        restored=1
        echo
        echo "==> restoring the console"
        if [ -e "$launcher" ]; then
            chmod 0755 "$launcher" || true
        fi
        rm -f /run/giochi/launcher-parado
        owner=""
        i=0
        while [ $i -lt 35 ]; do
            owner=$(drm_holders) || owner=""
            [ -n "$owner" ] && break
            i=$((i + 1))
            sleep 1
        done
        if [ -z "$owner" ]; then
            # A supervisor parked in `sleep 3600` needs the nudge; a running one does
            # not, which is why this is the fallback and not the first move.
            for pid in $(ps | grep '[g]iochi-init' | awk '{print $1}'); do
                kill -TERM "$pid" 2>/dev/null || true
            done
            i=0
            while [ $i -lt 35 ]; do
                owner=$(drm_holders) || owner=""
                [ -n "$owner" ] && break
                i=$((i + 1))
                sleep 1
            done
        fi
        echo "  launcher processes: $(ps | grep -c '[g]iochi-launcher' 2>/dev/null || echo 0)"
        if [ -n "$owner" ]; then
            echo "  DRM master: $owner $(cat "/proc/$owner/comm" 2>/dev/null) — console restored"
        else
            echo "  the console did NOT come back" >&2
            echo "  recover with: chmod +x $launcher; kill -TERM \$(ps | grep '[g]iochi-init' | awk '{print \$1}')" >&2
            exit 1
        fi
    fi
    exit "$status"
}
trap restore EXIT INT TERM

echo "==> releasing the console"
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
            giochi-launcher|giochi-racer) kill "-$signal" "$pid" 2>/dev/null || true;;
        esac
    done
}

for round in 1 2; do
    i=0
    while [ $i -lt 10 ]; do
        [ -z "$(drm_holders)" ] && break
        if [ "$round" = "1" ]; then kill_holders TERM; else kill_holders KILL; fi
        i=$((i + 1))
        sleep 1
    done
done

holders=$(drm_holders)
if [ -n "$holders" ]; then
    echo "  DRM still held by:" >&2
    for pid in $holders; do
        echo "    $pid $(cat "/proc/$pid/comm" 2>/dev/null)" >&2
    done
    exit 1
fi
released=1
echo "  console free (DRM master released)"

echo
echo "==> running the racer (${frames} frames)"
cd "$dest"
set +e
# `time` reports CPU seconds, which is the only headroom number a vsync-locked
# KMSDRM run can give: the wall clock is pinned to the panel's 60 Hz, so a frame that
# costs 4 ms and one that costs 16 ms both read 59.3 fps.
# shellcheck disable=SC2086
if [ "$auto" = "1" ]; then
    /usr/bin/time -f "  cpu: user %U s, sys %S s, wall %e s" \
        env SDL_VIDEODRIVER=kmsdrm RUST_LOG="$log_level" ./giochi-racer \
        --frames "$frames" --auto $extra \
        --quadro /tmp/racer-frame.bmp 2>&1
else
    /usr/bin/time -f "  cpu: user %U s, sys %S s, wall %e s" \
        env SDL_VIDEODRIVER=kmsdrm RUST_LOG="$log_level" ./giochi-racer \
        --frames "$frames" $extra \
        --quadro /tmp/racer-frame.bmp 2>&1
fi
run_status=$?
set -e
echo "  exit status: $run_status"

# Checked on the *host*, after the copy: the image has no python3, which is the same
# Buildroot minimalism that made `offscreen` unavailable.
if [ -f /tmp/racer-frame.bmp ]; then
    echo "==> capture"
    ls -l /tmp/racer-frame.bmp
else
    echo "  no frame was captured" >&2
fi

exit "$run_status"
REMOTE

echo
echo "==> pulling the capture"
if scp -q -o BatchMode=yes "$host:/tmp/racer-frame.bmp" "$local_frame" 2>/dev/null; then
    python3 giochi-racer/scripts/check-frame.py "$local_frame"
else
    echo "  no capture on the device" >&2
fi
