# Estado — GIOCHI-ENGINE

## Última fase concluída: 1 — Platform + Game Loop
Data: 2026-11-07

## Gates validados

Comando único: `scripts/gates.sh` (`SKIP_ARM64=1` pula o cross-check).

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --target x86_64-unknown-linux-gnu`
- [x] `cargo test --workspace --target x86_64-unknown-linux-gnu` — 66 testes, 0 falhas
- [x] `cargo check --workspace --target aarch64-unknown-linux-gnu`
- [x] `cargo doc --workspace --no-deps --document-private-items`

Verificações manuais da fase:

- `cargo run -p giochi-demo -- --frames 60` abre janela 1280x720 (driver `x11`
  após fallback), roda 60 iterações / 60 passos fixos a 59,6 fps reais e sai com
  0. Idem com `SDL_VIDEODRIVER=wayland` e com `SDL_VIDEODRIVER=x11`.
- `SDL_VIDEODRIVER=kmsdrm cargo run -p giochi-demo -- --frames 1` falha com
  mensagem descritiva listando `by-path, card1, renderD128` e os hints KMSDRM.
- `tests/platform_smoke.rs` (driver `dummy`, sem display): init explícito, janela
  320x200, título, rejeição de janela 0x200 com `INVALID_ARGUMENT`, e caminho
  completo ESC → evento SDL → `QuitSignal` → `GameLoop` parado.

## Entregue na fase 1

- `platform/sdl.rs`: `Platform` (Rc do contexto SDL: vídeo, timer, events, pump),
  `init_sdl()` que define `SDL_VIDEODRIVER=kmsdrm` **somente** se a variável
  estiver ausente, hints KMSDRM documentados como constantes, fallback explícito
  para o driver de desktop (log `warn` + `VideoDriverChoice::AutoFallback`) e erro
  descritivo com diagnóstico de `/dev/dri` quando KMSDRM falha de vez.
  Utilitários: `poll_events`, `push_event`, `delay_ms`, `ticks_ms`,
  `video_driver_name`, `video_driver_choice`, `has_drm_device`, `quit_signal`.
- `platform/window.rs`: `Window` real sobre `sdl2::video::Window` (fullscreen
  desktop, position centered, `size`, `drawable_size`, `set_title`,
  `is_fullscreen`), `WindowConfig` C-shaped validado (rejeita dimensão zero).
- `core/time.rs`: `Time::advance` com saneamento de `NaN`/negativo/infinito,
  `frame_count` saturante e `FrameClock` (Instant/CLOCK_MONOTONIC) — 6 testes.
- `core/game_loop.rs`: acumulador de passo fixo 1/60, clamp `MAX_FRAME_TIME=0.25`
  (relaxado para `max(0.25, fixed_timestep)`), `alpha()` para interpolação,
  `sleep_ms(work_time)` para o limitador de frame sem double-count,
  `with_fixed_timestep` validado e `handle_quit_signal` (Quit/Escape) — 14 testes.
- `giochi-demo`: janela preta 1280x720, loop fixo, ESC/fechar encerram, flag
  `--frames N` para execução determinística, saída com `ExitCode` e relatório de
  frames/fps — 5 testes de parsing.
- `tests/platform_smoke.rs`: integração headless com o driver `dummy`.
- `docs/DECISIONS.md`: ADR-015 (negociação de driver KMSDRM + fallback) e ADR-016
  (contexto SDL em `Rc` para `Window` sobreviver ao `Platform`).
- `docs/C_ABI_CONTRACT.md`: seção 5.2 documenta a negociação de driver e os hints.

## Próxima fase: 2 — Render 2D (SDL2 render, C-shaped)

`HandleRegistry<T>` passa a ser usado de fato; `TextureRegistry` com
`create_texture`/`destroy_texture` e contador de vivos; `RenderContext` com
`clear`, `draw_rect`, `draw_texture`, `present`; `Camera::world_to_screen`
implementada e testada; `math` com testes; exemplos `hello_rect` e
`moving_sprite`. Aceite: exemplos rodam em x86_64, create+destroy sem leak pelo
contador, câmera testada.

**Ao iniciar a fase 2:** remover `#![allow(dead_code)]` de `handle_registry.rs` e
o `#[allow(dead_code)]` de `Window::sdl_window`, que existem só até o primeiro
consumidor real.

## Bloqueios / pendências

- Nenhum bloqueio de gate.
- CMake do ambiente é 4.2.3 e o SDL2 vendorizado declara
  `cmake_minimum_required(VERSION 3.0.x)`; workaround em `.cargo/config.toml`
  (`CMAKE_POLICY_VERSION_MINIMUM=3.5`, ADR-014). Revisitar quando `sdl2-sys`
  subir o mínimo de CMake.
- O SDL2 vendorizado é compilado com KMSDRM habilitado (`SDL_KMSDRM=ON`,
  `SDL_KMSDRM_SHARED=ON` → `libdrm`/`gbm`/`EGL` via `dlopen` em runtime). Isso foi
  verificado nos objetos do build ARM64 (`SDL_kmsdrm*.c.o` são ELF aarch64), e é o
  que permite validar o cross-check sem sysroot ARM64 completo. Para link
  estático de produção, ver `docs/CROSS_COMPILE.md`.
- Nesta máquina o `kmsdrm` não inicializa (sessão gráfica detém DRM master); é
  exatamente o caso coberto pelo fallback do ADR-015.

## Notas de arquitetura

- `src/error.rs` não estava na estrutura original do prompt; foi adicionado como
  módulo de topo para não violar as regras de dependência (ADR-011).
- `EngineConfig.title` é `[u8; 64]` em vez de `String` (ADR-007).
- `TextureRegistry::create_texture` recebe só o path; o `TextureCreator` fica
  dentro do registro (ADR-008).
- `KeyCode = u32` com valores `SDLK_*` (ADR-009).
- ABI v1 é singleton: nada de `GiochiRenderContext*` público (ADR-012).
- `core` não importa `sdl2`: `GameLoop` recebe `QuitSignal` (definido em
  `platform`) em vez de `sdl2::event::Event`. `input::Input::update` receberá
  eventos na fase 3, ainda via `platform`.
- `Platform::poll_events`/`push_event` expõem `sdl2::event::Event` na camada
  `platform`, que é declaradamente o wrapper de SDL2 (e a fase 3 exige
  `Input::update(&[Event])`).
- Fase 6 é dona de `.github/workflows/*`, dos templates de issue/PR e do README
  completo; o diretório existe vazio de propósito.
