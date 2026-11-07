# Estado — GIOCHI-ENGINE

## Última fase concluída: 0 — Bootstrap + Contrato C ABI
Data: 2026-11-06

## Gates validados

Comando único: `scripts/gates.sh` (executa todos abaixo; `SKIP_ARM64=1` pula o último).

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --target x86_64-unknown-linux-gnu`
- [x] `cargo test --workspace --target x86_64-unknown-linux-gnu` — 31 testes, 0 falhas
- [x] `cargo check --workspace --target aarch64-unknown-linux-gnu`
- [x] `cargo doc --workspace --no-deps --document-private-items`

Binário: `cargo run -p giochi-demo` → imprime `giochi-engine bootstrap OK`.

## Entregue na fase 0

- Workspace de 4 crates: `giochi-engine`, `giochi-engine-capi` (`rlib` +
  `cdylib` + `staticlib`), `giochi-engine-lua` (mLua/lua54 vendored),
  `giochi-demo`.
- Árvore de módulos completa com assinaturas stub (`todo!("fase N")` documentado)
  para `core`, `platform`, `input`, `render`, `assets`, `audio`, `scene`, `math`
  e `handle_registry` (crate-private).
- `docs/C_ABI_CONTRACT.md`: `GIOCHI_ABI_VERSION 1`, forma C projetada de
  `GiochiVec2` (24 B), `GiochiRect` (32 B), `GiochiColor` (20 B),
  `GiochiSprite` (56 B), `GiochiCamera` (60 B), `GiochiTime` (32 B),
  `GiochiEngineConfig` (112 B), `GiochiSceneCallbacks` (56 B), handles `u64`,
  tabela de códigos de erro e tabela completa de funções `giochi_*` com a fase
  em que cada uma nasce.
- `docs/DECISIONS.md`: 14 ADRs (workspace, handles, modelo de erros, Lua,
  2D-only, `#[repr(C)]` + `_reserved`, buffer de título, registro de texturas,
  teclas como `u32`, single-thread, `src/error.rs`, ABI singleton, wrappers com
  `_reserved`, workaround do CMake 4).
- `tests/abi_layout.rs` (10 testes) trava tamanho/alinhamento de cada tipo do
  contrato.
- `HandleRegistry<T>` com handles não reutilizáveis, `NULL_HANDLE = 0` e 7 testes.
- `GiochiError` (thiserror) + `codes` (`i32`), com teste de unicidade e sinal.
- `scripts/gates.sh`, `scripts/check-aarch64.sh`,
  `cmake/aarch64-linux-toolchain.cmake` e `docs/CROSS_COMPILE.md`.
- `README.md`, `CONTRIBUTING.md`, `CHANGELOG.md`, `LICENSE`, `rust-toolchain.toml`,
  `rustfmt.toml`, `clippy.toml`, `Cross.toml`, `.editorconfig`, `.gitignore`.

## Próxima fase: 1 — Platform + Game Loop

`platform/sdl.rs` (init de subsistemas, `SDL_VIDEODRIVER=kmsdrm` apenas se
ausente, hints documentados, erro descritivo para `/dev/dri` inacessível),
`platform/window.rs` (wrapper `Window` real), `core/time.rs` (`Time` funcional +
testes), `core/game_loop.rs` (fixed timestep 1/60, acumulador, clamp de 0.25 s,
Quit/Escape) e `giochi-demo` abrindo janela preta 1280x720 que fecha com ESC.
Aceite: demo abre/fecha em x86_64 e `cargo check` ARM64 continua passando.

## Bloqueios / pendências

- Nenhum bloqueio de gate.
- Nota de ambiente (não é bloqueio): o CMake desta máquina é 4.2.3 e o SDL2
  vendorizado declara `cmake_minimum_required(VERSION 3.0.x)`. O workaround
  (`CMAKE_POLICY_VERSION_MINIMUM=3.5`) está em `.cargo/config.toml` e explicado
  no ADR-014. Revisitar quando `sdl2-sys` subir o mínimo de CMake.
- O cross-check ARM64 exige `gcc-aarch64-linux-gnu`; sem sysroot ARM64 completo o
  SDL2 compila sem ALSA/udev (suficiente para validar compilação). Ver
  `docs/CROSS_COMPILE.md`.
- `handle_registry.rs` tem `#![allow(dead_code)]` temporário: nenhum consumidor
  existe até a fase 2. **Remover junto com a primeira textura.**

## Notas de arquitetura

- `src/error.rs` não estava na estrutura original do prompt; foi adicionado como
  módulo de topo para não violar as regras de dependência (`math`/`render` não
  podem depender de `core`). Registrado no ADR-011 e já em `docs/DECISIONS.md`.
- `EngineConfig.title` é `[u8; 64]` em vez de `String` (ADR-007).
- `TextureRegistry::create_texture` recebe só o path; o `TextureCreator` fica
  dentro do registro (ADR-008).
- `KeyCode = u32` com valores `SDLK_*` (ADR-009).
- ABI v1 é singleton: nada de `GiochiRenderContext*` público (ADR-012).
- `giochi_abi_version()` e `giochi_engine_version()` já existem na capi (fase 0);
  o resto da superfície `extern "C"` nasce nas fases 1–7.
- Fase 6 é dona de `.github/workflows/*` (CI/release/cross-compile), dos templates
  de issue/PR e do README completo; o diretório existe vazio de propósito.
