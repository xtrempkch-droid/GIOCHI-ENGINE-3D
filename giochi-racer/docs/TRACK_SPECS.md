# Track specifications

A track is a **`.scene` file the editor reads and writes** — the same format as
`giochi-editor/assets/scenes/default.scene`. Nothing in this document describes code: it
describes the data the game expects to find in the file, so a track can be built and
changed in the visual editor without touching Rust (master plan, R3).

The shipped track, `assets/tracks/track_easy.scene`, follows every rule below and is
checked against them by `tests/track_specs.rs` on every `cargo test`.

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

## 3. Building or changing a track in the editor

1. `cargo run -p giochi-editor --release`.
2. `File > Open…` → `giochi-racer/assets/tracks/track_easy.scene`, or start from a new
   scene for a new track.
3. Build the surface from cube primitives scaled flat (`scale.y` ≈ `0.4`), keeping every
   top face at the same height.
4. Add the barriers the same way, taller than the road.
5. Add an empty named `Start` and set its position and heading with the rotate gizmo.
6. Add empties named `Checkpoint 1`, `Checkpoint 2`, … in lap order. Leave their scale at
   1 for a 12 m gate; change `scale.x` for a different radius.
7. `File > Save`. Do **not** rename the file unless you also change the path passed to the
   game (`--track` or `GameConfig::track`).
8. `cargo test -p giochi-racer --test track_specs` — it measures the file and compares
   against the numbers in this document. If you changed the circuit on purpose, update the
   table above and the test's `TRACKS` array in the same commit.

`scripts/verify-editor-package.sh` is not involved: the editor is the tool, the scene is
the artefact.

---

## 4. What phases 42 and 43 will add

The plan asks for three tracks and, from phase 43, waypoints for the AI. The format
already carries all of it, and only the reader grows:

| Phase | Track | Spec |
|---|---|---|
| 42 | `track_medium.scene` | figure-of-eight, 400 m, 6 checkpoints |
| 42 | `track_hard.scene` | S-curve with one ramp, 600 m, 8 checkpoints |
| 43 | all three | 10–20 nodes named `Waypoint N`, each with a `speed_target` |

Waypoints will be read by name with the same rule as checkpoints, and the target speed
will come from a second convention to be decided then (most likely `scale.x` in km/h,
which is the same trick as the gate radius and needs no new field). The AI driver itself
is `giochi-racer/src/ai/`, phase 43.

---

## 5. Checklist the tests enforce

`tests/track_specs.rs`, on every shipped track:

- the scene loads, and the name matches the table above;
- `Start` and the right number of checkpoints exist, each with a finite, unit, horizontal
  arrival direction;
- the centreline perimeter is the specified length (200 m for the oval);
- the road is 10 m wide between the barrier faces;
- every drivable slab's top face is at `y = 0` and every barrier sits on it;
- the car starts facing along the first straight;
- a lap can be counted, gate by gate, in order, three times;
- the track has a sun, and its scenery bounds fit inside the barriers plus runoff.

`tests/physics_track.rs` then drives the **real car with the real colliders** on the same
file: it starts on the road, accelerates along its heading, cannot fall through the floor,
cannot drive through a barrier, stays upright, brakes from 120 km/h, respawns, and
completes a full lap under a scripted driver.
