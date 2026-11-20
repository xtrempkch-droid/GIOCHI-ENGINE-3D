# Track specifications

A track is a **`.scene` file the editor reads and writes** — the same format as
`giochi-editor/assets/scenes/default.scene`. Nothing in this document describes code: it
describes the data the game expects to find in the file, so a track can be built and
changed in the visual editor without touching Rust (master plan, R3).

Three circuits ship: `track_easy.scene` (the phase-41 oval), `track_medium.scene` (a
figure of eight) and `track_hard.scene` (a serpentine with a hump). All three follow every
rule below and are checked against them by `tests/track_specs.rs`, which also *drives* a
full lap of each (`tests/lap_logic.rs`).

---

## 1. How a scene becomes a circuit

The game reads **node names**. There is no new component, because the scene format
belongs to the engine and the editor (R2), and a name is something the editor already
lets you edit.

| Node name | Meaning | Required |
|---|---|---|
| `Start` (also `StartPosition`, `StartFinish`, `Spawn`, `SpawnPoint`) | places the car, and **is** the start/finish line | yes |
| `Checkpoint 1` … `Checkpoint 9` | the gates of a lap, taken in numeric order | no |
| any node with a mesh | drawn; a primitive is also solid | at least one |
| any node with a light | the track's sun (the first one wins) | no |

Names are matched case-insensitively, ignoring spaces, `_` and `-`, so `checkpoint_2`,
`Checkpoint 2` and `CHECKPOINT2` are the same gate. A `Checkpoint` with no number is
numbered in the order it appears in the file.

**Markers are not scenery.** A node named `Start` or `Checkpoint N` is a point in space:
it is not drawn and it is not solid, even if it has a mesh. That is what lets you put a
gate on the racing line without putting a box there.

### The car's heading

The `Start` node's rotation sets the car's heading, read as a rotation about `+Y`:

- rotation `[0, 0, 0, 1]` (identity) → the car faces **`+Z`**;
- the quaternion for a quarter turn about `Y` (`[0, 0.7071, 0, 0.7071]`) → the car faces
  **`+X`**.

Only the Y component is read. Use the editor's gizmo to rotate the node in the viewport
rather than typing quaternions.

### The gate radius

A gate's radius is `scale.x` of the node when that scale is not `1`, and **12 m**
otherwise. So:

- drop a `Checkpoint 3` empty and leave its scale alone → a 12 m disc;
- set its scale X to `8` → an 8 m disc, for a gate that has to be hit precisely;
- set it to `20` → a generous gate on a wide straight.

(The **start line** always uses the 12 m default: it is what the car spawns inside, and a
smaller disc there would let a car start outside its own finish line. `Start`'s `scale` is
ignored, not read as a radius.)

The scale is read from the node's *world* transform, so a gate inside a scaled group is
scaled with it.

### The direction a gate has to be crossed in

Derived, not authored. Gate `i`'s direction is the vector from gate `i-1` to gate `i`,
with the finish line's "previous" being the last checkpoint. In other words: **the order
of the gates defines the lap**, and the game only scores a gate when the car is moving
along it. Driving the circuit backwards scores nothing; reversing over the line after a
spin does not score either, and does not invalidate the lap.

The consequence for the author: gates must be placed **in lap order and around the
circuit**. Two gates on top of each other give a degenerate direction, which falls back to
the car's starting heading.

### What is solid

| Node | Collider |
|---|---|
| `cube` primitive | a box, exactly `size × scale` and rotated with the node |
| `sphere` primitive | a ball of `radius × max(scale)` |
| `plane` primitive | a 1 m thick slab hanging **below** the surface, so the top face is where the plane is drawn |
| `.obj` / `.gltf` node | **nothing** — scenery is not a wall |

An imported model is drawn but not solid, because its bounds are only known after it is
loaded and a collider guessed from a file name would be worse than none. To make imported
scenery solid, wrap it in invisible cubes (a node with a mesh is solid; give it a
transparent material or place it under the floor).

---

## 2. The shipped track: `OVAL EASY`

A flat oval, 200 m round the centreline, 10 m wide between the barriers, four
checkpoints at the corners, and the start on the south straight facing east.

```
        z = +20.3  ┌───────────────────────────────────────────────┐
   outer barrier   │                                               │
        z = +20    │        ┌─────────────────────────────┐        │
                   │        │      infield (grass)        │        │
        z = +10    │        └─────────────────────────────┘        │
                   │                                               │
   centreline +15  │  = = = = =  road, 200 m round  = = = = =      │
                   │                                               │
        z = -10    │        ┌─────────────────────────────┐        │
                   │        │      infield (grass)        │        │
        z = -20    │        └─────────────────────────────┘        │
   outer barrier   │                                               │
        z = -20.3  └───────────────────────────────────────────────┘
                  x=-40.3                                  x=+40.3
```

| Property | Value |
|---|---|
| Centreline rectangle | `x = ±35`, `z = ±15` |
| Lap length round the centreline | `2·(70 + 30) = 200 m` |
| Road width (barrier face to barrier face) | `10 m` |
| Road surface | one flat plane at `y = 0`; every slab's top face is exactly `0` |
| Barrier height | `1.2 m`, sitting on the road |
| Start | `(0, 0.5, -15)`, facing `+X` |
| Checkpoints | `(35, -15)`, `(35, 15)`, `(-35, 15)`, `(-35, -15)` — clockwise from the start |
| Gate radius | 12 m |

### Why the ground is nine slabs and not one

The road, the infield and the four outfield pieces are separate boxes whose **top faces
are all at `y = 0`** and which **do not overlap in plan**. A single ground plane with the
road painted on top would either z-fight (coincident faces) or leave a 5 cm lip the car
trips over. Nine flush slabs have neither problem, and they are still seven draw calls
because the road is one shape and each grass area is another.

---

## 3. `FIGURE EIGHT` — `track_medium.scene`

Two 45 × 40 m loops whose inner sides are two diagonals crossing at the origin. Six
checkpoints, all on straights, so a gate disc never overlaps a different part of the
route.

```
        z = +20    ┌──────────────┐   ╱   ┌──────────────┐
                   │  left loop    ╲ ╱      right loop  │
        z =   0    │      west      ╳        east       │
                   │  (x = -50)    ╱ ╲    (x = +50)     │
        z = -20    └──────────────┘   ╲   └──────────────┘
                  x=-50        x=-5   0   x=+5        x=+50
```

| Property | Value |
|---|---|
| Loop rectangles | `x ∈ [-50, -5]` and `x ∈ [5, 50]`, both `z ∈ [-20, 20]` |
| Crossing | two diagonals: `(-5,-20)→(5,20)` and `(5,-20)→(-5,20)` |
| Lap length round the centreline | ≈ `414 m` (measured by the driven lap: 33 s at ~12 m/s) |
| Road width | `10 m` |
| Start | `(-40, 0.5, -20)`, facing `+X` |
| Checkpoints | `(-17.5,-20)`, `(27.5,20)`, `(50,0)`, `(27.5,-20)`, `(-27.5,20)`, `(-50,0)` |
| Gate radius | 12 m |

**The crossing has no walls and no road of its own beyond the two diagonals.** A barrier
along a diagonal cuts across every road it meets, which in a figure of eight is the inside
of the other loop — the car was fenced out of the crossing until those walls were removed.
Off the crossing the surface is grass, so a car that misses the diagonal can drive back;
the checkpoints, not the barriers, are what make the lap honest.

## 4. `SERPENTINE HARD` — `track_hard.scene`

Four 120 m straights joined by three U-turns, plus a return leg offset 15 m west so it does
not overlap them, and a hump on the second straight.

| Property | Value |
|---|---|
| Straights | `z = -60, -20, 20, 60`, `x ∈ [-60, 60]` |
| U-turns | `x = 60` (from `z=-60`), `x = -60` (from `z=-20`), `x = 60` (from `z=20`) |
| Return | `x = -75`, `z ∈ [-60, 60]`, joined by two 15 m stubs at `z = ±60` |
| Lap length | ≈ `640 m` (measured: 76 s of driven lap) |
| Road width | `10 m` |
| Start | `(-45, 0.5, -60)`, facing `+X` |
| Checkpoints | `(0,-60)`, `(60,-40)`, `(-25,-20)`, `(0,20)`, `(60,40)`, `(0,60)`, `(-75,-40)`, `(-67.5,-60)` |
| Gate radius | 12 m |

### The hump

Three slabs on the second straight, all 10 m wide:

| Piece | Span in `x` | Top surface |
|---|---|---|
| up-slope | `-15 … 0` | `0 → 1.0 m`, rotated `+3.81°` about `Z` |
| flat top | `0 … 10` | `1.0 m` |
| down-slope | `10 … 25` | `1.0 m → 0`, rotated `−3.81°` about `Z` |

`rise / run = 1 / 15`, so the angle is `atan(1/15)`. A steeper ramp is a wall the car hits;
this one is driven over at full speed and never launches the car, which is what "ramp" has
to mean on an arcade model with no suspension.

### Why the walls stop short of a junction

Every wall box is `road width` shorter than its road and centred on it, so the last 5 m
before each junction is open. Without that, one segment's barrier fences the car out of
the next turn: the first version of this circuit was undrivable at three of its four
corners, and the driven-lap test is what found it.

---

## 5. Building or changing a track in the editor

1. `cargo run -p giochi-editor --release`.
2. `File > Open…` → one of the three files, or start from a new scene for a new track.
3. Build the surface from cube primitives scaled flat (`scale.y` ≈ `0.4`), keeping every
   top face at the same height.
4. Add the barriers the same way, taller than the road, and leave a gap at every junction.
5. Add an empty named `Start` and set its position and heading with the rotate gizmo.
6. Add empties named `Checkpoint 1`, `Checkpoint 2`, … in lap order. Leave their scale at
   1 for a 12 m gate; change `scale.x` for a different radius.
7. `File > Save`. Do **not** rename the file unless you also change the path passed to the
   game (`--track` or `GameConfig::track`).
8. `cargo test -p giochi-racer --test track_specs --test lap_logic` — the first measures
   the file against the tables above, the second drives a lap of it. If the circuit changed
   on purpose, update the table, the test's `TRACKS`, and `ROUTES` in the same commit.

`scripts/generate-tracks.py` wrote the two phase-42 circuits as data. It is not the way to
edit them: once a track has been opened and saved in the editor, the generator is stale and
should be updated or deleted. Keeping it is what makes the geometry reviewable as
arithmetic, and it is why a wall in the wrong place can be fixed in one line.

---

## 6. What phase 43 adds

Waypoints for the AI, read by name with the same rule as checkpoints: 10–20 nodes named
`Waypoint N` per circuit, with the target speed in `scale.x` (km/h), the same trick as the
gate radius. The AI driver is `giochi-racer/src/ai/`; the pure-pursuit controller in
`tests/lap_logic.rs` is its prototype and already drives all three circuits.

---

## 7. Checklist the tests enforce

`tests/track_specs.rs`, on every shipped track:

- the scene loads, and the name matches the table above;
- `Start` and the right number of checkpoints exist, each with a finite, unit, horizontal
  arrival direction, and a radius of at least 12 m unless authored smaller;
- the oval's centreline perimeter is 200 m (its gates are its corners);
- the road is 10 m wide between the barrier faces;
- every drivable slab's top face is at `y = 0` and every barrier sits on it;
- the car starts facing along the first straight;
- a lap can be counted, gate by gate, in order, three times;
- the track has a sun, every primitive is solid, and the play area is at least 100 m across.

`tests/lap_logic.rs` then drives the **real car with the real colliders** around every
circuit under a pure-pursuit controller, and cuts a lap on purpose to prove the lap is
invalidated. `tests/physics_track.rs` covers the phase-41 behaviours (starts on the road,
cannot fall through the floor or drive through a barrier, stays upright, brakes from
120 km/h, respawns).
