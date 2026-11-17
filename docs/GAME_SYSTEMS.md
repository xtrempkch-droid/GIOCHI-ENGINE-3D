# Game systems: nav, cameras, saves, audio buses and i18n

Five pieces the Production Pack added for the game (phases 41–45), and the reasoning behind
each. They are small on purpose: every one of them is arithmetic or file handling with no new
dependency, so a game can use one without dragging in the other four.

| System | Where | Feature |
|---|---|---|
| A\* on a grid | `giochi_engine::nav` | none |
| Follow / shake / cutscene cameras | `giochi_engine_3d::camera` | none |
| Save slots | `giochi_engine::save` | none |
| Audio buses | `giochi_engine::audio` (`Bus`) | none |
| Translations | `giochi_engine::i18n` | none |
| glTF/GLB models | `giochi_engine_3d::mesh::load_gltf` | none — see [3D.md](3D.md) |

---

## 1. A\* pathfinding (`nav`)

```rust
use giochi_engine::{GridPoint, NavGrid, PathOptions};

let grid = NavGrid::from_rows(&[
    "....",
    ".##.",
    "....",
]);
let path = grid.find_path(GridPoint::new(0, 0), GridPoint::new(3, 2));  // Some(cells)
let waypoints = grid.smooth(&path.unwrap());                           // corners only
```

| Setting | Default | What it changes |
|---|---|---|
| `diagonals` | `true` | Octile movement; `false` is Manhattan |
| `prevent_corner_cutting` | `true` | Refuses a diagonal that slips between two blocked cells |
| `max_expansions` | `40 000` | Turns a hopeless search into `None` instead of a freeze |

Three properties worth knowing:

- **Deterministic.** Two equal-cost routes are broken by `(f, h, y, x)`, so the same query
  always returns the same cells — a replay or a test never flakes on a tie.
- **Admissible heuristic.** Octile (diagonals) or Manhattan (orthogonal), both scaled by the
  step costs, so the first time the goal is popped the path is optimal.
- **`line_of_sight` agrees with the search.** Smoothing cannot produce a leg A\* would have
  refused, because both apply the same corner rule.

`NavGrid::from_rows` reads a map from ASCII (`#`/`X` blocked), which is how the tests draw
their maps and how a game can load a level's collision layer without a second format.

---

## 2. Cameras (`giochi_engine_3d::camera`)

```rust
let mut camera = CameraDirector::new();
camera.update(player_position, player_velocity, dt);
camera.shake.add_trauma(0.6);          // on a hit
camera.play(cutscene);                 // takes over until it ends
let view = camera.to_camera3d(width, height);
```

- **`FollowCamera`** trails a point behind and above the target, looking ahead along its
  velocity. The convergence is `1 - exp(-stiffness · dt)` — the exact solution of a
  first-order lag — so the camera behaves the same at 30 and 144 fps and can never overshoot.
  `snap` is for respawns: easing across the map after a teleport is a camera the player
  watches instead of playing.
- **`ShakeCamera`** is trauma that decays, mapped to an offset by `trauma²` and a hash-based
  noise. No `rand` dependency, and the same replay shakes the same way. The look-at point
  shakes less than the eye, because shaking the aim as much as the viewpoint makes a scene
  unreadable.
- **`CutsceneCamera`** is keys with per-segment easing (`Linear`, `SmoothStep`, `EaseIn`,
  `EaseOut`). Sampling outside the key range clamps to the ends instead of extrapolating.
- **`CameraDirector`** picks: a playing cutscene wins, the shake is added on top, and the
  follow camera keeps updating during a cutscene so the return is seamless.

Every one of them is pure `f32` math with no GL and no window, which is why the tests run
without a driver.

---

## 3. Save slots (`save`)

```rust
let saves = SaveSlots::new("saves");
saves.save(SaveRequest::new(2, &json)
    .with_name("Depois da ponte")
    .with_playtime(123.5))?;
for info in saves.list() { /* draw a row: name, time, playtime */ }
let game = saves.load(2)?.payload;   // the game's own JSON, byte for byte
```

The payload is **opaque**: the engine stores whatever string the game hands it and returns it
unchanged. What the engine owns is the part everybody gets wrong:

| Rule | Why |
|---|---|
| Atomic write (`.tmp` + rename) | a power cut leaves the old save or the new one, never half |
| `payload_bytes` in the header | a payload may contain `---`, or a line that looks like a header |
| Version check | a save from a newer build is refused with both numbers, not half-read |
| Slot number, not a path | `slot-3.save`, built from the number: no path traversal |
| Header carries name and playtime | a load menu draws its rows without parsing the payload |

`SLOT_COUNT` is 8. `first_free`, `newest` and `delete_all` exist because every save menu ends
up writing them.

---

## 4. Audio buses (`audio::Bus`)

```rust
let mut audio = AudioSystem::new()?;
audio.set_bus_volume(Bus::Music, 40);       // quieter soundtrack
audio.play_sound_on(Bus::Sfx, engine, 0)?;  // this one on the SFX bus
audio.stop_bus(Bus::Sfx);                   // pause menu: engine stops, music does not
```

A bus is a **multiplier on the master**, not a replacement: a channel on bus `b` plays at
`master · b / 128`, so the master at `0` still mutes everything. `Bus::ALL` is master, SFX,
music and UI; the numbers are pinned in `docs/C_ABI_CONTRACT.md` §5.6 because
`giochi_audio_set_bus_volume` takes them.

The channel volume is set **after** `SDL_mixer` allocates it, and the bus is recorded per
channel, which is what makes a volume change apply to sounds already playing and lets
`stop_bus` leave the other buses alone.

---

## 5. Translations (`i18n`)

```rust
let mut t = Translator::new(Language::English);
t.load_folder("assets/i18n")?;
t.set_language(Language::Portuguese);
let text = t.format("hud.lap", &[("current", "2"), ("total", "5")]);
assert_eq!(text, "Volta 2 de 5");
```

A catalogue is a `key = value` text file (`assets/i18n/en.txt`, `pt.txt`, `es.txt`), because
the engine has no `serde` and a JSON parser for a flat list would be three times the code.

- Missing key → fallback language → the key itself. Showing `menu.quit` is ugly; showing an
  empty button is a bug the player cannot report.
- `{name}` substitution is **one pass**: a value that contains a placeholder is not expanded
  again, so a translation cannot recurse.
- `Translator::missing_keys(other, reference)` is the translator's check:
  `tests/i18n_catalogues.rs` runs it over the shipped files, and also compares the
  **placeholders** of every string, which is the mistake that otherwise ships silently.
- `Language::from_code("pt_BR")` accepts a full locale and looks at the primary subtag.

---

## 6. What is still missing

Nothing from the original Production Pack list. The known limits are stated where they live:

| Limit | Where it is written |
|---|---|
| glTF textures and normal maps are not read (factors only) | `docs/3D.md` §3.3, ADR-073 |
| No skeletal animation | `model_viewer`'s module docs |
| Pathfinding is grid-only, no navmesh | this document, §1 |
| No spatial audio (buses are volume only) | `docs/C_ABI_CONTRACT.md` §5.6 |
