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
# Two states it honours instead of fighting:
#
#   * `/run/giochi/dev-free-console` — while that file exists the supervisor parks the
#     launcher on purpose, so the console is *already* free and has to stay that way.
#     Re-enabling the launcher there would bring a UI up in the middle of somebody
#     else's session; the script reports and leaves it alone.
#   * a **different unit on the same address**. The USB gadget serves one console at a
#     time and every reflash makes a new host key, so `known_hosts` reports a "man in
#     the middle" that is really just the other handheld. `--refresh-host-key` says
#     "I know, it is my console"; without it the script explains that instead of
#     dumping the SSH banner.
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
#   giochi-racer/scripts/device-run.sh --refresh-host-key      # another unit on the IP
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
track=""
log_level=""
binary="target/aarch64-unknown-linux-gnu/release/giochi-racer"
skip_deploy=0
auto=1
refresh_host_key=0
launcher="/opt/giochi/apps/launcher/bin/giochi-launcher"
local_frame="/tmp/racer-frame.bmp"

usage() {
    cat <<'USAGE'
Deploy the racer to a console over SSH, run it on the real display, and hand the
screen back exactly as it was found.

  --host USER@HOST     SSH destination (default root@10.55.0.2)
  --dest DIR           deployment directory on the device (default /opt/racer)
  --frames N           frames to run (default 600)
  --size WxH           window size passed to the game (KMSDRM uses the panel's mode)
  --laps N             laps in the race
  --track PATH         track to race, relative to the game's assets/ (default easy)
  --log LEVEL          RUST_LOG level (default info)
  --binary PATH        ARM64 binary to deploy
  --launcher PATH      the console's launcher, the process holding DRM master
  --skip-deploy        reuse the binary already on the device
  --keyboard           do not script the driver: read the pad (or the keyboard)
  --refresh-host-key   forget the host key; a different unit answers on that IP
  -h, --help           this text

The script releases DRM master, runs the game, pulls the capture back, checks it and
restores the console from a trap *on the device*, so a dropped SSH connection cannot
leave it black. It leaves a console that was freed with /run/giochi/dev-free-console
alone, and it refuses to deploy a binary whose glibc floor is above the device's.
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --host) host="$2"; shift 2;;
        --dest) dest="$2"; shift 2;;
        --frames) frames="$2"; shift 2;;
        --size) size="$2"; shift 2;;
        --laps) laps="$2"; shift 2;;
        --track) track="$2"; shift 2;;
        --log) log_level="$2"; shift 2;;
        --binary) binary="$2"; shift 2;;
        --launcher) launcher="$2"; shift 2;;
        --skip-deploy) skip_deploy=1; shift;;
        --keyboard) auto=0; shift;;
        --refresh-host-key) refresh_host_key=1; shift;;
        -h|--help) usage; exit 0;;
        *) echo "unknown argument '$1'; --help" >&2; exit 2;;
    esac
done

hostname_only="${host#*@}"
ssh_opts=(-o BatchMode=yes -o ConnectTimeout=10)

if [[ "$refresh_host_key" == "1" ]]; then
    echo "==> forgetting the host key for $hostname_only (a different unit answers there)"
    ssh-keygen -R "$hostname_only" >/dev/null 2>&1 || true
    # ...and trusting the one that is there now. `ssh-keygen -R` only removes: without
    # the scan below, `BatchMode=yes` refuses the connection with "Host key verification
    # failed", which the error path below then misreports as a lost `authorized_keys`.
    # Measured on 2026-11-20 on the R35S.
    if ! ssh-keyscan -T 10 "$hostname_only" 2>/dev/null >>"$HOME/.ssh/known_hosts"; then
        echo "error: could not read the new host key from $hostname_only" >&2
        exit 1
    fi
fi

# A reflashed console is a new host key at the same address. Turning that into one
# sentence and a command is the difference between "the tool is broken" and "plugged in
# the other handheld".
if ! probe="$(ssh "${ssh_opts[@]}" -o ConnectTimeout=8 "$host" true 2>&1)"; then
    if grep -q "HOST IDENTIFICATION HAS CHANGED" <<<"$probe"; then
        cat >&2 <<'EOF'
error: the host key changed. The gadget carries one console at a time and every image
       reflash generates a new key, so this is almost always the other unit rather than
       an attack. If the console now plugged in is the one you meant, rerun with:
           --refresh-host-key
EOF
    else
        cat >&2 <<'EOF'
error: cannot log in. A reflashed image loses /root/.ssh/authorized_keys, so the key
       has to be installed again with the device password (giochi-dev on the dev image):
           ssh root@HOST   # password, then append ~/.ssh/id_ed25519.pub
EOF
        echo "  ssh said: $probe" >&2
    fi
    exit 1
fi

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
    # The host's glibc is newer than the consoles'. A binary linked here demands the
    # host's symbol versions and dies on the device with `version 'GLIBC_x' not found`,
    # which neither `cargo check` nor the cross-compile CI job can see (they never link
    # against the console's libc). Refuse to deploy one.
    floor="$(aarch64-linux-gnu-readelf -sW --dyn-syms "$binary" 2>/dev/null |
        grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sort -Vu | tail -1 || true)"
    device_glibc="$(ssh "${ssh_opts[@]}" "$host" 'ldd --version 2>/dev/null | head -1' |
        grep -oE '[0-9]+\.[0-9]+$' || true)"
    if [[ -n "$floor" && -n "$device_glibc" ]]; then
        echo "==> glibc: binary needs ${floor#GLIBC_}, device has $device_glibc"
        if [[ "$(printf '%s\n%s\n' "${floor#GLIBC_}" "$device_glibc" | sort -V | tail -1)" != "$device_glibc" ]]; then
            echo "error: $binary needs glibc ${floor#GLIBC_}, the device has $device_glibc." >&2
            echo "       Build it in a container at the device's glibc -- docs/CROSS_COMPILE.md" >&2
            echo "       section \"The host's glibc is the ceiling, not the device's\"." >&2
            exit 1
        fi
    fi

    echo "==> deploying to $host:$dest"
    # The path is expanded here on purpose: it is this side's idea of the destination.
    # shellcheck disable=SC2029
    ssh "${ssh_opts[@]}" "$host" "mkdir -p '$dest/assets'"
    scp -q -o BatchMode=yes "$binary" "$host:$dest/giochi-racer"
    scp -q -o BatchMode=yes -r giochi-racer/assets/. "$host:$dest/assets/"
fi

# ---------------------------------------------------------------------------
# The device side. One script, one trap, both the release and the restore.
# ---------------------------------------------------------------------------
args="$frames $auto $dest $launcher ${size:--} ${laps:--} ${log_level:-info} ${track:--}"
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
track="$8"
extra=""
[ "$size" != "-" ] && extra="$extra --size $size"
[ "$laps" != "-" ] && extra="$extra --laps $laps"
# Absolute on the device: the game resolves a relative `--track` against the executable
# and the working directory, and the deployment directory is neither `assets/`.
[ "$track" != "-" ] && extra="$extra --track $dest/$track"

DEV_FREE=/run/giochi/dev-free-console
# While this file exists the supervisor parks the launcher on purpose: the console is
# free because somebody made it free, not because something is wrong.
dev_free() { [ -e "$DEV_FREE" ]; }

panel="$(head -1 /sys/class/drm/card0-*/modes 2>/dev/null || true)"
rotate="$(cat /sys/class/graphics/fbcon/rotate 2>/dev/null || echo '?')"
echo "==> device: $(hostname), $(nproc) cores, $(awk '/MemTotal/{printf "%d MB", $2/1024}' /proc/meminfo)"
echo "  $(uname -m) kernel $(uname -r), panel ${panel:-unknown}, fbcon rotate $rotate"
echo "  $(ldd --version 2>/dev/null | head -1)"

# Every process holding card0. renderD128 is not reported: the render node does not
# block a KMS surface, and a launcher that opened it once is not worth failing over.
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
        if dev_free; then
            # Bringing the launcher back here would interrupt whoever freed the console.
            echo "  $DEV_FREE is present: left free, launcher still parked"
            exit "$status"
        fi
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
if dev_free; then
    echo "  already free ($DEV_FREE)"
elif [ -e "$launcher" ]; then
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
            giochi-launcher|giochi-racer|giochi-gamepad) kill "-$signal" "$pid" 2>/dev/null || true;;
            *) echo "  leaving '$comm' ($pid) alone: not ours to kill" >&2;;
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
