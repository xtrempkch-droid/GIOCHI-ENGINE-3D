# Roadmap

What is planned, in the order the plan puts it, and what is deliberately not planned. The
**state** of every phase is in [PROGRESS.md](PROGRESS.md); this file answers "what comes
next and why", so a contribution can be aimed at something that will be accepted.

Phases 0–41 are delivered. What follows is the Salto C (GIOCHI RACER) tail.

## Delivered, in one line each

| Phases | What |
|---|---|
| 0–18 | Engine core: SDL2 platform, fixed timestep, 2D renderer, input, assets, audio, scenes, C ABI parts I–III, batching/instancing/upscaling |
| 19–24 | Growth Pack: hot reload, devtools console, widget UI, particles, animation, Rapier physics, C ABI v6 |
| 25–29 | Releases 0.5.0/0.5.1: release CI, networking wire v2, `mdns-sd` 0.21, `egui` 0.33.3 |
| 30–40 | **Salto B — `giochi-editor`**: viewport, hierarchy, inspector, asset browser, gizmos, undo/redo, autosave, play preview, standalone package |
| — | **Production Pack**: glTF/GLB, PBR-lite materials, scene format, save slots, game cameras, A\*, audio buses, i18n |
| 41 | **Salto C — `giochi-racer`**: car, track, chase camera, HUD, measured on R35S and x55 |

## Next

### Phase 42 — three tracks, checkpoints, laps

Today there is **one** track (`giochi-racer/assets/tracks/track_easy.scene`, a 200 m oval)
and one `LapTracker`. The phase adds two more circuits and the content around them.

- Tracks are data, in the editor's `.scene` format; the convention (`Start`,
  `Checkpoint N`, …) is in [giochi-racer/docs/TRACK_SPECS.md](giochi-racer/docs/TRACK_SPECS.md)
  and is asserted by `tests/track_specs.rs`.
- Acceptance: each track drivable by the physics car in a test, ordered gates, an invalidated
  lap for a cut course.

### Phase 43 — opponent AI

- The engine already has grid A\* (`giochi_engine::nav`); whether the opponents use it or
  steering depends on whether the tracks stay corridors — see the navmesh note in
  [IDEAS.md](IDEAS.md).
- Acceptance: at least one opponent that completes laps and can be overtaken, deterministic
  enough to test headless.

### Phase 44 — menus, persistence, polish

- Pause overlay (today `Esc`/Start **quit**), settings, best-lap persistence through
  `giochi_engine::save::SaveSlots`.
- Polish: the pieces measured at 41 that a player notices — camera feel, HUD readability.
- Acceptance: quit and resume with the race state intact; a settings screen that changes what
  the game loop consumes.

### Phase 45 — DRS and Quality/Performance modes

- Dynamic resolution rides the existing upscaler (`RenderContext3D::set_render_scale`); the CPU
  budget measured in phase 41 leaves ~65 % headroom.
- **Panel rotation is scheduled here**: `giochi_engine::platform::panel_rotation` already reads
  `/sys/class/graphics/fbcon/rotate`; what is missing is the presentation step — render into a
  rotated target and blit, or set the primary plane's `rotation` property. The measurement and
  both options are in [docs/KMSDRM.md §9](docs/KMSDRM.md).
- Acceptance: the game hits a frame-time target on the R35S by lowering resolution, and the
  x55 renders upright.

### Phase 46 — ARM64 build, GIOCHI-OS package, release

- The build must happen **at the device's glibc**, not the host's:
  `scripts/build-aarch64.sh --container` runs the same build in `debian:13-slim` and every
  build prints the glibc floor of each binary. See
  [docs/CROSS_COMPILE.md](docs/CROSS_COMPILE.md) §"The host's glibc is the ceiling".
- `giochi-editor` forwards `sdl2-system` (it did not before), so the editor can ship in the
  same package.
- The release of the game is a **tag**, not a new crate version: the workspace stays on the
  engine's number.
- Acceptance: installs and runs on both measured consoles with the same artefact.

## Known engineering debt

These are tracked because they will be hit by the phases above, not because they are ideas.

| Item | Where | What unblocks it |
|---|---|---|
| Panel rotation not applied | `giochi_engine::platform::rotation` + presentation pass | phase 45 |
| `giochi_ui_*` missing from the C ABI | the facade is already C-shaped | a C consumer that needs it |
| 3D HUD input (`translate_event` is `pub(crate)`, keyed on the 2D event type) | `giochi-engine/src/ui` | lifting the table into a shared crate |
| Physics 3D, contacts and raycast not in the C ABI | — | deciding how a `Vec` crosses the boundary (same question the net snapshots answered with fixed arrays) |
| No clipboard on KMSDRM | egui requests it, the platform has none | a platform-level clipboard |
| mDNS test is `#[ignore]`d | needs real multicast on an interface | run with `--ignored` on a LAN |

## Not planned

Recorded in [IDEAS.md](IDEAS.md) with the reason each one is out: skeletal 3D, tilemaps,
post-processing, asset streaming, Windows/macOS, split-screen, VR, car tuning, career mode.
They are not rejected for being bad; they are not this product.

## Contributing

[docs/../CONTRIBUTING.md](CONTRIBUTING.md) has the gates and the rules. A change that touches a
phase above should update that phase's row in [PROGRESS.md](PROGRESS.md) with **how it was
verified**, not only that it is done — the repository's convention is that a phase is closed by
a measurement, and several past bugs passed with every test green.
