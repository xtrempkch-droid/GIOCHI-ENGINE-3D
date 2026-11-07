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
5. **No `unsafe` outside `giochi-engine-capi`.** The core crate declares
   `#![forbid(unsafe_code)]`. Every `unsafe` block needs a `// SAFETY:` comment.
6. **2D only.** Do not introduce EGL, GLES, shaders, VBOs, meshes or render
   pipelines.

The authoritative reference for the external surface is
[docs/C_ABI_CONTRACT.md](docs/C_ABI_CONTRACT.md). If a type does not fit C, the
type is redesigned — the contract is not relaxed.

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

If a gate cannot pass for an environmental reason, document the blocker in
`PROGRESS.md` and leave the phase open. Do not mark it complete.

## Style

* `cargo fmt` decides formatting; do not hand-format.
* Public items need documentation: the crate warns on `missing_docs`.
* Comments explain *why*, not *what*. A comment that repeats the code is noise.
* Stubs for future phases use `todo!("fase N: ...")` and say so in their doc.

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
