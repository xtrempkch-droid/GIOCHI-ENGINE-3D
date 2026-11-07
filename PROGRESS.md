# Estado — GIOCHI-ENGINE

## Última fase concluída: 7 — C ABI (`giochi-engine-capi`)
Data: 2026-11-07

## Gates validados

Comando único: `scripts/gates.sh` (`SKIP_ARM64=1` pula o cross-check).

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --target x86_64-unknown-linux-gnu`
- [x] `cargo test --workspace --target x86_64-unknown-linux-gnu` — 189 testes, 0 falhas
- [x] `cargo check --workspace --target aarch64-unknown-linux-gnu`
- [x] `cargo doc --workspace --no-deps --document-private-items` (0 warnings)
- [x] `scripts/gen-header.sh --check` — `giochi.h` em sincronia com o crate

Verificações manuais da fase:

- `scripts/ffi-smoke.sh`: `libgiochi.so` construída, `c_smoke.c` compilado com
  `-Wall -Wextra -Werror` contra o header gerado, executado, e `python_smoke.py`
  (só `ctypes`, biblioteca padrão) executado. Ambos `OK (abi 1, engine 0.1.0)`.
- `cargo test -p giochi-engine-capi`: 6 testes unitários + 2 de integração
  (`rust_ffi.rs`), incluindo o fluxo completo de cena com `on_enter`/`update`/
  `render`/`on_exit`, quit por `giochi_engine_request_quit()`, dois `run_scene`
  seguidos com o mesmo handle, `step` e desmontagem idempotente.
- Cross ARM64 **com link** em container `ubuntu:24.04` limpo:
  `libgiochi.so: ELF 64-bit LSB shared object, ARM aarch64` além do `giochi-demo`.
- Smoke de runtime inalterado: `giochi-demo --verify`, `mini_game --auto` a 61 fps.

## Entregue na fase 7

- `giochi-engine-capi/src/` organizado em `abi_types`, `state`, `version`, `engine`,
  `render`, `input`, `audio`, `assets` e `scene` — 54 funções `#[no_mangle] extern "C"`.
- `abi_types.rs`: `GiochiEngineConfig` (`char title[64]`), `GiochiSceneCallbacks`
  (56 bytes), os 11 códigos de erro e `GIOCHI_MAX_ASSET_SIZE` como literais com
  `const _: () = assert!(...)` contra `giochi_engine::error::codes`.
- `state.rs`: singleton `thread_local!` (ADR-012), slot de erro por thread limpo na
  entrada de cada chamada, `guard` com `catch_unwind` em toda função exportada,
  acessores por subsistema com dica contextual, slots de callback e o flag de quit.
- `scene.rs`: registo de cenas com handles nunca reutilizados, adaptador `CScene`
  que instala os subsistemas Emprestados (ADR-028), `on_enter`/`on_exit` entregues
  fora do empréstimo do engine, e drenagem de saídas diferidas.
- `render.rs`, `input.rs`, `audio.rs`, `assets.rs`, `engine.rs`, `version.rs`: a
  superfície completa, com verificação de NULL e mensagens acionáveis.
- `giochi-engine-capi/cbindgen.toml` + `include/giochi.h` gerado (932 linhas,
  todos os tipos e funções com a documentação do crate).
- `scripts/gen-header.sh` (`--check` falha se o header estiver desatualizado) e
  `scripts/ffi-smoke.sh`.
- `giochi-engine-capi/tests/`: `rust_ffi.rs`, `c_smoke.c`, `python_smoke.py`.
- `docs/C_ABI.md`: guia de consumo, tabela "o que cada callback pode usar", handles,
  frame pacing, uso por outras linguagens e verificação.
- `docs/C_ABI_CONTRACT.md` atualizado: estado "completo", destruidores devolvem
  `int32_t`, tabela de disponibilidade por callback, funções de diagnóstico e de
  estatística, `giochi_texture_create_memory`, checklist §7 com o local de cada
  garantia.
- `docs/DECISIONS.md`: ADR-028 (callback usa exatamente o que o engine empresta) e
  ADR-029 (`Engine::step` partilha o corpo do frame com `run_current`).
- `docs/ARCHITECTURE.md`: secção 5b sobre a estrutura interna da capi.
- `README.md` com secção "From C", `CHANGELOG.md` com a fase 7, `CONTRIBUTING.md`
  com o header gerado, `ci.yml` com o job `ffi` e a verificação do header,
  `release.yml` a empacotar `include/giochi.h` + `lib/libgiochi.{so,a}`.
- Engine: `RenderContext` guarda o `TextureRegistry` em `Rc` e expõe
  `shared_textures()`; `TextureRegistry::create_texture_from_bytes`;
  `Engine::step(frame_time)` e o refactor `run_frame`/`FrameOutcome`.

## Próxima fase: 8 — Binding Lua (`giochi-engine-lua`)

Módulo `giochi` para Lua 5.4 via mLua (`giochi-engine-lua`): expor a API Rust
idiomática com handles `u64` (nunca ponteiros), `examples/mini_game.lua` como jogo de
referência, subcomando `giochi-demo lua <script>` e `docs/LUA_BINDING.md`.
Aceite: `mini_game.lua` corre headless pelo subcomando `lua` e o gate de testes
executa o script.

## Bloqueios / pendências

- Nenhum bloqueio de gate.
- CMake 4.2.3 × `cmake_minimum_required(VERSION 3.0.x)` do SDL2 vendorizado:
  workaround em `.cargo/config.toml` (ADR-014).
- `sdl2` compila com `unsafe_textures` (ADR-017); um único canvas por processo
  (ADR-018).
- O caminho "ao vivo" do gamepad não é exercitável sem hardware (ADR-020).
- Nesta máquina o `kmsdrm` não inicializa (sessão gráfica detém DRM master) — caso
  coberto pelo fallback do ADR-015. O dispositivo de áudio existe, então o caminho
  real de mixer foi verificado.
- `SceneTransition::Replace`/`Push` não podem ser aplicados pelo engine (o enum não
  carrega payload); a troca de cena é feita por `Engine::push_scene`/
  `replace_scene`. Documentado no ADR-025.
- O cross ARM64 **com link** exige os satélites SDL2 para aarch64
  (`libSDL2_image`/`libSDL2_mixer`/`libSDL2_ttf`): o `bundled` do crate `sdl2`
  compila só o SDL2 core. Nesta máquina eles não existem, então o gate da fase
  continua sendo `cargo check` e o link é verificado no CI (receita multiarch) e em
  container limpo. ADR-027.
- Até a fase 6 o alvo de produção não linkava (`cargo check` não linka): faltava o
  linker no `.cargo/config.toml` **e** os satélites ARM64. Corrigido e agora
  coberto pelo job `link` do `cross-compile.yml`.
- `giochi.h` é gerado pelo `cbindgen` 0.27: quem não o tiver instalado vê o gate do
  header ser saltado em `scripts/gates.sh`, mas o CI falha se estiver desatualizado.
- A ABI v1 não expõe pilha de cenas: `giochi_engine_run_scene` deixa a pilha vazia no
  fim, pelo que cada handle equivale a uma cena ativa. `push`/`pop`/`replace` são
  decisão da fase 8+ se Lua precisar de overlays.

## Notas de arquitetura

- `src/error.rs` não estava na estrutura original do prompt; módulo de topo para não
  violar as regras de dependência (ADR-011).
- `EngineConfig.title` é `[u8; 64]` (ADR-007); `KeyCode`/`MouseButton`/
  `GamepadAxis`/`GamepadButton` são `u32` com valores SDL (ADR-009); ABI v1
  singleton (ADR-012); wrappers públicos têm `_reserved` (ADR-013).
- Ordem obrigatória por frame: `poll_events()` → `input.update(&events)` →
  `game_loop.advance()` → `scene.update()` → `clear` → `scene.render()` →
  `present()` → `transition()` → `sleep`.
- `Engine` é o único dono de `Platform`/`RenderContext`/`AudioSystem`/`Input`/
  `SceneManager`; cenas recebem empréstimos por parâmetro e nunca guardam nada
  (ADR-024). O `Drop` do `Engine` limpa cenas, áudio, cache e texturas.
- `render` não conhece `scene` (a dependência é `core::engine` → `scene` →
  `render`/`input`/`audio`); `core` não importa `sdl2`; `math` não depende de
  módulos internos; `handle_registry` é `pub(crate)`.
- Próxima fase: 8 (binding Lua com `mini_game.lua` e subcomando `giochi-demo lua`).
- A capi não conhece `sdl2`: fala apenas com `giochi-engine`, e a única `unsafe` do
  workspace está lá, com `// SAFETY:` em cada bloco.
