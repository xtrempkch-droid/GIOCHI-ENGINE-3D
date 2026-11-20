#!/usr/bin/env python3
"""Write the racing line -- the `Waypoint` nodes -- into the shipped circuits.

Phase 43 needs the opponents to know where the road goes. The circuits already describe
their *scoring* volumes (`Start`, `Checkpoint N`), but a gate is not a line: the straight
between two gates crosses the inside of every corner, and a driver aiming at gates drives
into the barrier. The line is the centreline, corner included, so it is a separate set of
nodes.

Run this **after** `generate-tracks.py`, which rewrites the two generated scenes from
scratch and would otherwise drop the waypoints.

`--check` writes nothing and exits non-zero if a scene is out of date, which is what
`scripts/gates.sh` runs: the speeds below are computed from the car's handling, so a circuit
whose file was edited by hand (or not regenerated after a tuning change) has to fail the
build rather than quietly become undrivable.

# The line is corners, not a fillet

A racing line could be emitted as a dense arc-sampled spline -- 200 nodes per circuit. It is
not, for two reasons:

* **The controller already rounds the corners.** Pure pursuit aims at a point a fixed
  distance ahead along the polyline, so a right-angle vertex is driven as a sweep, not as a
  turn in place. Dense nodes buy nothing the look-ahead does not already give.
* **A human has to be able to edit them.** The editor story is phase 44; twelve markers a
  person can see and drag beats two hundred.

So one waypoint per corner, plus one in the middle of every long straight so the planner has
something to aim at down the middle of a 120 m run.

# The speed comes from the car, not from taste

Two numbers per circuit, both derived:

* **On a straight** the waypoint asks for [`STRAIGHT_KMH`], the fastest the car goes.
* **At a corner** it asks for the fastest the car can hold the sweep that pure pursuit will
  actually drive there. That sweep has a radius of `look_ahead / (2 sin(turn / 2))`: two
  chords of the look-ahead length meeting at the corner. The arcade model's yaw rate at
  speed `v` is `steer_rate * (1 - steer_falloff * v / max_speed)`, and a car of speed `v`
  turning at `w` follows `r = v / w`; solved for `v`:

      v = steer_rate * r / (1 + steer_rate * steer_falloff * r / max_speed)

  times a safety factor for grip loss. The constants are copied from
  `CarConfig::default()` and the look-ahead from `Route::DEFAULT_LOOK_AHEAD`;
  `tests/waypoints.rs` drives the result, so a stale constant fails the build instead of
  quietly making the opponents slower.
"""

import json
import math
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "assets" / "tracks"

# From `CarConfig::default()` in `giochi-racer/src/vehicle/car.rs`.
STEER_RATE = 1.7           # rad/s at full lock and low speed
STEER_FALLOFF = 0.55       # how much of the steering is lost at top speed
MAX_SPEED = 120.0 / 3.6    # m/s
# From `Route::DEFAULT_LOOK_AHEAD` in `giochi-racer/src/ai/waypoint.rs`.
LOOK_AHEAD = 10.0          # m

# How much of the theoretical limit an opponent is asked to use. The limit assumes perfect
# steering and no grip loss; a line driven at 100 % of it spends the race in the barrier.
SAFETY = 0.85
# Nobody crawls, and nobody exceeds the car's own top speed on a straight.
MIN_KMH = 25.0
STRAIGHT_KMH = 100.0
# A vertex that turns less than this is a straight with a wobble, not a corner.
CORNER_RADIANS = math.radians(12.0)
# Longest run between waypoints before one is added halfway, in metres.
MIDPOINT_EVERY = 45.0

MARK_MAT = {"base_color": [1.0, 1.0, 1.0, 1.0], "texture": None, "specular": 0.0,
            "shininess": 16.0, "emissive": [0.0, 0.0, 0.0], "roughness": 1.0, "metallic": 0.0}


def limit_ms(radius):
    """The fastest the arcade model can hold a turn of `radius` metres, in m/s."""
    radius = max(radius, 0.5)
    return STEER_RATE * radius / (1.0 + STEER_RATE * STEER_FALLOFF * radius / MAX_SPEED)


def corner_kmh(turn):
    """The speed asked for at a vertex that turns by `turn` radians."""
    if turn < CORNER_RADIANS:
        return STRAIGHT_KMH
    radius = LOOK_AHEAD / (2.0 * math.sin(turn / 2.0))
    return min(STRAIGHT_KMH, max(MIN_KMH, SAFETY * limit_ms(radius) * 3.6))


def turn_at(points, index):
    """The angle, in radians, by which the line turns at `index`."""
    count = len(points)
    prev, here, nxt = points[(index - 1) % count], points[index], points[(index + 1) % count]
    ax, az = here[0] - prev[0], here[1] - prev[1]
    bx, bz = nxt[0] - here[0], nxt[1] - here[1]
    la, lb = math.hypot(ax, az), math.hypot(bx, bz)
    if la < 1e-6 or lb < 1e-6:
        return 0.0
    return math.acos(max(-1.0, min(1.0, (ax * bx + az * bz) / (la * lb))))


def refine(corners):
    """`corners` with a midpoint added to every run longer than `MIDPOINT_EVERY`."""
    out = []
    count = len(corners)
    for index in range(count):
        x0, z0 = corners[index]
        x1, z1 = corners[(index + 1) % count]
        out.append((x0, z0))
        if math.hypot(x1 - x0, z1 - z0) > MIDPOINT_EVERY:
            out.append(((x0 + x1) / 2.0, (z0 + z1) / 2.0))
    return out


def waypoint_nodes(points, height):
    """One `Waypoint N` node per point, with the speed in `scale.x` as km/h."""
    nodes = []
    for index, (x, z) in enumerate(points):
        kmh = corner_kmh(turn_at(points, index))
        nodes.append({
            "id": None,
            "name": f"Waypoint {index + 1}",
            "parent": None,
            "transform": {"position": [round(x, 4), round(height(x, z), 4), round(z, 4)],
                          "rotation": [0.0, 0.0, 0.0, 1.0],
                          "scale": [round(kmh, 2), 1.0, 1.0]},
            # No mesh: a waypoint is data, like a gate. Drawing 200 markers would hide the
            # road it describes.
            "mesh": None,
            "material": dict(MARK_MAT),
            "light": None,
            "camera": None,
        })
    return nodes


def flat(_x, _z):
    return 0.5


def with_ramp(x, z):
    """The serpentine's 1 m hump, so a waypoint sits on the road rather than under it."""
    if abs(z + 20.0) > 5.0:
        return 0.5
    if -15.0 <= x <= 0.0:
        return 0.5 + (x + 15.0) / 15.0
    if 0.0 <= x <= 10.0:
        return 1.5
    if 10.0 <= x <= 25.0:
        return 1.5 - (x - 10.0) / 15.0
    return 0.5


# The centreline of each circuit: the corners, in lap order, starting at the start line.
# These are the same corners `tests/lap_logic.rs` used to prove the circuits drivable before
# waypoints existed. The last corner is *not* repeated at the end: the line is a closed loop,
# and `Route` wraps from the last waypoint back to the first.
CIRCUITS = [
    {
        "file": "track_easy.scene",
        "name": "OVAL EASY",
        "height": flat,
        "corners": [(0.0, -15.0), (35.0, -15.0), (35.0, 15.0), (-35.0, 15.0), (-35.0, -15.0)],
    },
    {
        "file": "track_medium.scene",
        "name": "FIGURE EIGHT",
        "height": flat,
        "corners": [
            (-40.0, -20.0), (-5.0, -20.0), (5.0, 20.0), (50.0, 20.0), (50.0, -20.0),
            (5.0, -20.0), (-5.0, 20.0), (-50.0, 20.0), (-50.0, -20.0),
        ],
    },
    {
        "file": "track_hard.scene",
        "name": "SERPENTINE HARD",
        "height": with_ramp,
        "corners": [
            (-45.0, -60.0), (60.0, -60.0), (60.0, -20.0), (-60.0, -20.0), (-60.0, 20.0),
            (60.0, 20.0), (60.0, 60.0), (-60.0, 60.0), (-75.0, 60.0), (-75.0, -60.0),
            (-60.0, -60.0),
        ],
    },
]


def main(argv):
    check = "--check" in argv
    stale = []
    for circuit in CIRCUITS:
        path = OUT / circuit["file"]
        before = path.read_text()
        doc = json.loads(before)
        if doc["name"] != circuit["name"]:
            raise SystemExit(f"{path}: expected {circuit['name']!r}, found {doc['name']!r}")

        # Idempotent: a re-run replaces the line instead of adding a second one.
        kept = [n for n in doc["nodes"] if not n["name"].startswith("Waypoint ")]
        dropped = len(doc["nodes"]) - len(kept)
        next_id = max((n["id"] for n in kept), default=0) + 1

        line = refine(circuit["corners"])
        nodes = waypoint_nodes(line, circuit["height"])
        for node in nodes:
            node["id"] = next_id
            next_id += 1
        kept.extend(nodes)

        doc["nodes"] = kept
        doc["next_id"] = next_id
        after = json.dumps(doc, indent=1) + "\n"

        speeds = [n["transform"]["scale"][0] for n in nodes]
        summary = (f"{path.name}: {len(nodes)} waypoints ({dropped} replaced), "
                   f"{min(speeds):.0f}-{max(speeds):.0f} km/h, "
                   f"{sum(1 for s in speeds if s == min(speeds))} at the slowest corner")
        if after == before:
            print(f"{summary} [up to date]")
            continue
        if check:
            stale.append(path.name)
            print(f"{summary} [STALE]")
            continue
        path.write_text(after)
        print(f"{summary} [written]")

    if stale:
        print(f"\n{', '.join(stale)}: run scripts/generate-waypoints.py", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
