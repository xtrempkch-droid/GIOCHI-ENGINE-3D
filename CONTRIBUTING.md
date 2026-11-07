# Contributing to GIOCHI-ENGINE

## Ground rules

1. **The public surface stays C-shaped.** No `pub` field may be `Rc`, `Arc`, `Box`,
   `String`, `Vec`, `HashMap`, a reference, or generic. No `pub fn` may have an
   explicit lifetime or generic parameters.
2. **Resources are handles.** Anything the user owns is a `u64` behind a
   crate-private registry, with an explicit destructor.
3. **Every public struct is `#[repr(C)]`** and ends with `_reserved: [u32; N]`,
   `N >= 4`. New fields must fit the reserved space or bump the ABI version.
4. **No `unwrap()` in library code.** Use `?` with `GiochiError`. `unwrap()` is
   acceptable in tests and examples.
5. **No `unsafe` outside `giochi-engine-capi`.** The core and the Lua binding declare
   `#![forbid(unsafe_code)]`. Every `unsafe` block in the C ABI needs a `// SAFETY:`
   comment.
6. **2D only.** Do not introduce EGL, GLES, shaders, VBOs, meshes or render
   pipelines.
7. **Bindings do not change the engine.** `giochi-engine-capi` and
   `giochi-engine-lua` are consumers: if a binding needs something the core cannot
   express, that is a core change, reviewed on its own, not a fork of behaviour.

The authoritative reference for the external surface is
[docs/C_ABI_CONTRACT.md](docs/C_ABI_CONTRACT.md). If a type does not fit C, the
type is redesigned — the contract is not relaxed.

## Environment

```bash
sudo scripts/install-deps.sh          # native build dependencies (Debian/Ubuntu)
sudo scripts/install-deps.sh --cross  # adds the ARM64 cross toolchain
```

One script, used by every workflow. Do not copy its package list into a CI job: an
incomplete copy is invisible until the build fails on a machine that lacks what your
machine happens to have, which is exactly how the CI stayed red through phases 6–8.

## Workflow

1. Read `PROGRESS.md` and pick the next phase. One phase per session.
2. Read `docs/C_ABI_CONTRACT.md` and `docs/DECISIONS.md` before writing code. Do
   not silently contradict an ADR: supersede it with a new one.
3. Implement, adding tests in the same phase as the functionality.
4. Run every gate:

```bash
scripts/gates.sh
```

5. Update `PROGRESS.md` with the phase, the gates and the next step.
6. Commit as `feat(phase-N): <summary>` (or `wip(phase-N): ...` for partial work).

## Gates

| Gate | Command |
|---|---|
| Formatting | `cargo fmt --all --check` |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` |
| Build | `cargo build --workspace --target x86_64-unknown-linux-gnu` |
| Tests | `cargo test --workspace --target x86_64-unknown-linux-gnu` |
| Docs | `cargo doc --workspace --no-deps --document-private-items` |
| ARM64 | `scripts/check-aarch64.sh` |
| C header | `scripts/gen-header.sh --check` |
| Version consistency | `scripts/check-version.sh` |
| Asset twins | `python3 scripts/png-to-bmp.py --check assets/*.png` |
| Scripts | `shellcheck scripts/*.sh` |
| C/Python consumers | `scripts/ffi-smoke.sh` |
| Real console (manual) | `scripts/hardware-check.sh --host root@<device>` |

If a gate cannot pass for an environmental reason, document the blocker in
`PROGRESS.md` and leave the phase open. Do not mark it complete.

The same gates run in GitHub Actions ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)),
plus a `smoke` job that executes the demo, every example and the Lua reference game
headlessly (`SDL_VIDEODRIVER=dummy`), and an `ffi` job that compiles `c_smoke.c` against
the generated header and runs the Python consumer.

[`cross-compile.yml`](.github/workflows/cross-compile.yml) adds the job `ci.yml` cannot
afford on every push: a real ARM64 **link** with the multiarch SDL2 packages.
[`release.yml`](.github/workflows/release.yml) packages both targets for a tag.

All three call the same scripts rather than reimplementing them, so a fix lands once:
`scripts/install-deps.sh`, `scripts/install-arm64-deps.sh`, `scripts/check-aarch64.sh`, `scripts/build-aarch64.sh`,
`scripts/gen-header.sh`, `scripts/ffi-smoke.sh`, `scripts/gates.sh`, `scripts/png-to-bmp.py`,
`scripts/hardware-check.sh`, `scripts/check-version.sh`.

## Style

* `cargo fmt` decides formatting; do not hand-format.
* Public items need documentation: the crate warns on `missing_docs`.
* Comments explain *why*, not *what*. A comment that repeats the code is noise.
* Stubs for future phases use `todo!("fase N: ...")` and say so in their doc.

## Documentation

`docs/API_GUIDE.md` is executable: every code block is also a test in
`giochi-engine/tests/api_guide.rs`. When you change an example in the guide you
must change the matching block in that file, and vice versa — the gate fails if
they drift, which is deliberate.

| Document | Update it when |
|---|---|
| `docs/C_ABI_CONTRACT.md` | Any public type or function changes (mandatory) |
| `giochi-engine-capi/include/giochi.h` | Never by hand: run `scripts/gen-header.sh` |
| `docs/C_ABI.md` | The C consumption workflow or the callback rules change |
| `docs/LUA_BINDING.md` | The Lua module, a capability or a handle lifetime changes |
| `docs/ARCHITECTURE.md` | A module is added, removed, or changes layer |
| `docs/API_GUIDE.md` + `tests/api_guide.rs` | The user-facing API changes |
| `docs/KMSDRM.md` | Driver negotiation or target requirements change |
| `docs/DECISIONS.md` | A non-obvious choice is made, or an ADR is superseded |
| `CHANGELOG.md` | Anything user-visible changes |
| `PROGRESS.md` | A phase starts, finishes, or is blocked |


## Releasing

`Cargo.toml` is the single source of truth for the version. A bump touches six
files, and `scripts/check-version.sh` (part of `gates.sh` and of CI) fails when any
of them disagrees:

| File | What it must say |
| --- | --- |
| `Cargo.toml` | the new `version` |
| the three member manifests | the new `version` in each `path = ..., version = ...` pin |
| `giochi-engine/src/lib.rs` | the matching `VERSION_PATCH` (a unit test ties it to `CARGO_PKG_VERSION`) |
| `giochi-engine-capi/include/giochi.h` | regenerated by `scripts/gen-header.sh`; never hand-edited |
| `docs/C_ABI_CONTRACT.md`, `docs/LUA_BINDING.md`, `README.md` | the literal version |
| `CHANGELOG.md` | a `## [x.y.z] - YYYY-MM-DD` section |

Then:

```bash
bash scripts/gates.sh          # includes check-version.sh and the header check
git commit -am "chore(release): x.y.z"
git tag -a vx.y.z -m "GIOCHI-ENGINE x.y.z"
git push origin main vx.y.z    # the tag triggers release.yml
```

`release.yml` builds both targets with LTO, smokes the release binary, packages
`giochi-demo`, `giochi.h`, `libgiochi.{so,a}`, the Lua reference game, the docs and
`assets/` (both the PNGs and their BMP twins), then attaches the archives to the
release. It creates a **draft** on purpose: check the artefacts before publishing.

Publishing a draft that still contains known defects is worse than no release — 0.1.0
stayed in draft precisely because a hardware run afterwards found four portability
bugs in it.

## Commits

```
feat(phase-2): 2D renderer with u64 texture handles

- HandleRegistry<T> with non-reused handles
- TextureRegistry::create_texture/destroy_texture
- RenderContext clear/draw_rect/draw_texture/present
- examples hello_rect, moving_sprite
```

Keep the subject line under 72 characters. Reference the ABI version bump if the
change touches the contract.
