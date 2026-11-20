#!/usr/bin/env python3
"""Writes the two circuits that phase 41 did not have, in the editor's scene format.

    python3 giochi-racer/scripts/generate-tracks.py

The output is `track_medium.scene` and `track_hard.scene` next to this script's
`../assets/tracks/`, overwriting them. Nothing else reads this file: the game loads the
`.scene` files, so they can be re-authored in the visual editor and this script must then
be updated or deleted — `tests/lap_logic.rs` is what notices if they disagree.

Why a generator at all: R3 asks for tracks built in the editor, and an agent cannot drive
a GUI. The files below *are* editor-format scenes (same `format`/`version`/`nodes`), so
opening one in `giochi-editor` and saving it is a normal edit. The generator exists so the
geometry is reviewable as arithmetic instead of 40 hand-written JSON nodes, and so a
mistake in a wall position can be fixed in one place.

The two circuits are laid out as a sum of rectangles and two diagonals:

* **FIGURE EIGHT** — two 45x40 loops whose east/west sides are the two diagonals that
  cross at the origin. No road and no wall on the inner sides: a wall there cuts the
  crossing exactly where the car must pass (learned by driving it, see `lap_logic.rs`).
* **SERPENTINE HARD** — four 120 m straights joined by three U-turns, a return leg offset
  west so it does not overlap them, and a 2 m hump on the second straight: a 15 m
  up-slope, a 10 m flat top and a 15 m down-slope, all in 0.4 m slabs.

Wall boxes stop half a road-width short of each end so a junction is open; without that,
one segment's barrier fences the car out of the next turn.
"""

import json, math, pathlib

OUT = pathlib.Path(__file__).resolve().parent.parent / 'assets' / 'tracks'

ROAD_MAT = {"base_color": [0.17, 0.18, 0.2, 1.0], "texture": None, "specular": 0.18,
            "shininess": 24.0, "emissive": [0.0, 0.0, 0.0], "roughness": 0.86, "metallic": 0.0}
GRASS_MAT = {"base_color": [0.19, 0.33, 0.16, 1.0], "texture": None, "specular": 0.05,
             "shininess": 8.0, "emissive": [0.0, 0.0, 0.0], "roughness": 0.95, "metallic": 0.0}
WALL_MAT = {"base_color": [0.82, 0.82, 0.86, 1.0], "texture": None, "specular": 0.25,
            "shininess": 24.0, "emissive": [0.0, 0.0, 0.0], "roughness": 0.7, "metallic": 0.0}
MARK_MAT = {"base_color": [1.0, 1.0, 1.0, 1.0], "texture": None, "specular": 0.0,
            "shininess": 16.0, "emissive": [0.0, 0.0, 0.0], "roughness": 1.0, "metallic": 0.0}

def q_y(angle):
    return [0.0, math.sin(angle / 2), 0.0, math.cos(angle / 2)]

def q_z(angle):
    return [0.0, 0.0, math.sin(angle / 2), math.cos(angle / 2)]

def node(nid, name, pos, scale, rot=(0.0, 0.0, 0.0, 1.0), mat=None, mesh=True):
    return {"id": nid, "name": name, "parent": None,
            "transform": {"position": [round(v, 4) for v in pos],
                          "rotation": [round(v, 6) for v in rot],
                          "scale": [round(v, 4) for v in scale]},
            "mesh": {"primitive": {"kind": "cube", "size": 1.0}} if mesh else None,
            "material": mat or ROAD_MAT, "light": None, "camera": None}

class Builder:
    def __init__(self, name):
        self.name = name
        self.nodes = []
    def add(self, *args, **kw):
        self.nodes.append(node(len(self.nodes) + 1, *args, **kw))
    def road(self, label, cx, cz, length, width=10.0, angle=0.0, y=0.02):
        # A slab whose local +X runs along the segment.
        self.add(label, (cx, y - 0.2, cz), (length, 0.4, width), q_y(angle))
    def walls(self, label, cx, cz, length, width=10.0, angle=0.0, height=1.2, trim=None):
        # Two walls, one on each side of the road, stopping half a road-width short of
        # each end. Without the gap the wall of one segment crosses the mouth of the next
        # at every junction, and the car is fenced out of the turn it is supposed to take
        # (found by the autopilot test in `tests/lap_logic.rs`).
        trimmed = length - (width if trim is None else trim)
        if trimmed <= 0.5:
            return
        dx, dz = math.cos(angle), -math.sin(angle)
        px, pz = -dz, dx              # perpendicular to the road direction
        for side, sign in (("L", 1.0), ("R", -1.0)):
            off = sign * (width / 2 + 0.5)
            self.add(f"{label} Wall {side}",
                     (cx + px * off, height / 2, cz + pz * off),
                     (trimmed, height, 0.5), q_y(angle), WALL_MAT)
    def gate(self, name, x, z, yaw=0.0):
        # Scale stays 1: the loader reads `scale.x` as the radius *in metres* when it is
        # not 1, and always gives the start line the default 12 m disc.
        self.add(name, (x, 0.5, z), (1.0, 1.0, 1.0), q_y(yaw), mesh=False, mat=MARK_MAT)
    def terrain(self, size, y_top=0.0):
        self.add("Terrain", (0.0, y_top - 0.2, 0.0), (size, 0.4, size), mesh=True, mat=GRASS_MAT)
    def sun(self):
        n = node(len(self.nodes) + 1, "Sun", (0.0, 20.0, 0.0), (1.0, 1.0, 1.0), mesh=False, mat=MARK_MAT)
        n["light"] = {"direction": [-0.45, -1.0, -0.35], "color": [1.0, 0.98, 0.94, 1.0],
                      "ambient": 0.32, "specular": 0.35, "shininess": 40.0}
        self.nodes.append(n)
    def save(self, path):
        doc = {"format": "giochi-scene", "version": 1, "name": self.name,
               "next_id": len(self.nodes) + 1, "nodes": self.nodes}
        with open(path, "w") as f:
            json.dump(doc, f, indent=1)
            f.write("\n")

def diagonal(b, label, x0, z0, x1, z1, width=10.0):
    dx, dz = x1 - x0, z1 - z0
    length = math.hypot(dx, dz)
    angle = math.atan2(-dz, dx)
    cx, cz = (x0 + x1) / 2, (z0 + z1) / 2
    b.road(label, cx, cz, length, width, angle)
    # No walls on the crossing: a wall along a diagonal cuts across every road it meets,
    # and in a figure-eight that is the middle of the other loop. The crossing is an open
    # apron on purpose; the checkpoints, not the barriers, are what make the lap honest
    # (found by `tests/lap_logic.rs`).

# ---------------------------------------------------------------- medium: figure 8
m = Builder("FIGURE EIGHT")
m.terrain(300.0)
# Left loop: x -50..-5, z -20..20
m.road("Left South", -27.5, -20, 45); m.walls("Left South", -27.5, -20, 45)
m.road("Left North", -27.5, 20, 45);  m.walls("Left North", -27.5, 20, 45)
m.road("Left West", -50, 0, 40, angle=math.pi / 2);  m.walls("Left West", -50, 0, 40, angle=math.pi / 2)
# No `Left East`: the east side of the left loop *is* the two diagonals. A road there
# plus its walls closes the loop and walls off the crossing.
# Right loop: x 5..50, z -20..20
m.road("Right South", 27.5, -20, 45); m.walls("Right South", 27.5, -20, 45)
m.road("Right North", 27.5, 20, 45);  m.walls("Right North", 27.5, 20, 45)
# No `Right West`, for the same reason.
m.road("Right East", 50, 0, 40, angle=math.pi / 2);  m.walls("Right East", 50, 0, 40, angle=math.pi / 2)
# The crossing: two diagonals through the origin
diagonal(m, "Cross Up", -5, -20, 5, 20)
diagonal(m, "Cross Down", 5, -20, -5, 20)
m.gate("Start", -40, -20, math.pi / 2)
m.gate("Checkpoint 1", -17.5, -20)
m.gate("Checkpoint 2", 27.5, 20)
m.gate("Checkpoint 3", 50, 0)
m.gate("Checkpoint 4", 27.5, -20)
m.gate("Checkpoint 5", -27.5, 20)
m.gate("Checkpoint 6", -50, 0)
m.sun()
m.save(str(OUT / "track_medium.scene"))

# ---------------------------------------------------------------- hard: serpentine + ramp
h = Builder("SERPENTINE HARD")
h.terrain(400.0)
xs, xw = 60.0, -60.0
rows = [-60.0, -20.0, 20.0, 60.0]
labels = ["Straight A", "Straight B", "Straight C", "Straight D"]
for row, label, direction in zip(rows, labels, ["east", "west", "east", "west"]):
    h.road(label, 0, row, 120)
    h.walls(label, 0, row, 120)
# U-turns at both ends (40 m each), alternating
h.road("Turn One", xs, -40, 40, angle=math.pi / 2); h.walls("Turn One", xs, -40, 40, angle=math.pi / 2)
h.road("Turn Two", xw, 0, 40, angle=math.pi / 2);   h.walls("Turn Two", xw, 0, 40, angle=math.pi / 2)
h.road("Turn Three", xs, 40, 40, angle=math.pi / 2); h.walls("Turn Three", xs, 40, 40, angle=math.pi / 2)
# Return leg, offset west so it does not overlap the U-turns: x=-75 from z=60 to z=-60
h.road("Return Leg", -75, 0, 120, angle=math.pi / 2); h.walls("Return Leg", -75, 0, 120, angle=math.pi / 2)
h.road("Return Stub North", -67.5, 60, 15); h.walls("Return Stub North", -67.5, 60, 15)
h.road("Return Stub South", -67.5, -60, 15); h.walls("Return Stub South", -67.5, -60, 15)

# The ramp: a 15 m up-slope, a 10 m flat top and a 15 m down-slope on straights B/C area.
rise, slope = 1.0, 15.0
theta = math.atan2(rise, slope)
hyp = math.hypot(slope, rise)
h.add("Ramp Up", (-7.5, 0.02 - 0.2 + rise / 2, -20), (hyp, 0.4, 10), q_z(theta))
h.add("Ramp Top", (5.0, 0.02 - 0.2 + rise, -20), (10.0, 0.4, 10))
h.add("Ramp Down", (17.5, 0.02 - 0.2 + rise / 2, -20), (hyp, 0.4, 10), q_z(-theta))

h.gate("Start", -45, -60, math.pi / 2)
h.gate("Checkpoint 1", 0, -60)
h.gate("Checkpoint 2", 60, -40)
h.gate("Checkpoint 3", -25, -20)
h.gate("Checkpoint 4", 0, 20)
h.gate("Checkpoint 5", 60, 40)
h.gate("Checkpoint 6", 0, 60)
h.gate("Checkpoint 7", -75, -40)
h.gate("Checkpoint 8", -67.5, -60)
h.sun()
h.save(str(OUT / "track_hard.scene"))

for p in (str(OUT / "track_medium.scene"), str(OUT / "track_hard.scene")):
    d = json.load(open(p))
    print(p, "nodes", len(d["nodes"]), "gates", sum(1 for n in d["nodes"] if "heckpoint" in n["name"] or n["name"] == "Start"))
