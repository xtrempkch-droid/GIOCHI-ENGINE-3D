# Estado — GIOCHI-ENGINE

## Última fase concluída: 5 — Scene + Engine (fachada)
Data: 2026-11-07

## Gates validados

Comando único: `scripts/gates.sh` (`SKIP_ARM64=1` pula o cross-check).

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --target x86_64-unknown-linux-gnu`
- [x] `cargo test --workspace --target x86_64-unknown-linux-gnu` — 182 testes, 0 falhas
- [x] `cargo check --workspace --target aarch64-unknown-linux-gnu`
- [x] `cargo doc --workspace --no-deps --document-private-items` (sem warnings)

Verificações manuais da fase:

- `cargo run -p giochi-demo -- --verify` (x11): 1 frame pelo loop do `Engine`,
  `texturas vivas: 1, sons vivos: 1, áudio: ativo`.
- `cargo run -p giochi-engine --example mini_game -- --frames 600` (sem input):
  368 frames, **fim de jogo** com pontuação 0 — o caminho game over → `Quit`
  funciona e para o loop.
- `cargo run -p giochi-engine --example mini_game -- --auto --frames 600`:
  600 frames completos, **pontuação final 120** — tiro, colisão e pontuação
  comprovados; jogo sobrevivível e jogável.
- `tests/engine_smoke.rs` (driver `dummy`): pilha vazia → push (`on_enter`) →
  push → pop (`on_exit`) → replace (`exit`+`enter`); loop real com frame limit de
  40 (40 renders, `updates == stats.steps`); `Quit` para o loop e deixa
  `is_running() == false`; pilha vazia roda e desenha; sem handles vazando.

## Entregue na fase 5

- `scene/scene.rs`: `trait Scene` com `on_enter`/`on_exit`/`update(Time, &Input,
  &mut AudioSystem)`/`render(&mut RenderContext)`/`transition()`;
  `SceneTransition` (`Continue`/`Quit`/`Replace`/`Push`/`Pop`) e `EmptyScene`
  (contador de callbacks para testes) — 4 testes.
- `scene/state.rs`: `SceneManager` com `push`/`pop`/`replace`/`clear`/`current`/
  `current_mut`/`depth`/`is_empty`, chamando `on_enter`/`on_exit` exatamente uma vez
  por cena — 10 testes.
- `core/engine.rs`: `Engine` (fachada) dono de `Platform`, `RenderContext`,
  `AudioSystem`, `Input`, `SceneManager`, `GameLoop`, relógio e `RunStats`;
  `Engine::new(config)`, `with_scene`, `run`, `run_current`, `push_scene`,
  `replace_scene`, `pop_scene`, `request_quit`, `is_running`, `reset_quit`,
  `set_frame_limit`/`clear_frame_limit`, `run_stats`, `set_clear_color`,
  acessores (`render`, `audio`, `input`, `scenes`, `resources`, `platform`,
  `game_loop`, `fixed_timestep`) e `Drop` que limpa cenas/áudio/texturas —
  14 testes.
- `RunStats` C-shaped (`frames`, `steps`, `elapsed`, `_reserved`) com `fps`,
  `step_rate` e `frame_time_ms`.
- `giochi-demo` refatorado para usar `Engine` + `Scene`: sprite móvel com WASD,
  arrasto com botão direito, companheiro que quica, bipe no ESPAÇO, grid de fundo,
  marcador de "áudio carregado" e flags `--frames N` / `--verify`.
- `examples/mini_game.rs`: space shooter completo (ship, drones, balas,
  colisões, vidas, pontuação, dificuldade progressiva, game over, som procedural de
  tiro) usando todos os módulos; `--frames N` e `--auto` (auto-play para verificação
  por script).
- `tests/engine_smoke.rs`: integração headless do loop e da pilha de cenas.
- `docs/DECISIONS.md`: ADR-024 (engine empresta subsistemas; cenas não os possuem),
  ADR-025 (transições como pedido aplicado após o frame), ADR-026 (loop único com
  frame limit no `Engine`).
- `docs/C_ABI_CONTRACT.md`: contrato de callbacks (quando `update`/`render`/`on_enter`/
  `on_exit` são chamados), lifetime de cena em execução e semântica de transições.

## Próxima fase: 6 — Polimento + CI + Docs

`.github/workflows/{ci,release,cross-compile}.yml`, `.github/ISSUE_TEMPLATE/`,
`PULL_REQUEST_TEMPLATE.md`, `dependabot.yml`; `docs/ARCHITECTURE.md`,
`docs/KMSDRM.md`, `docs/API_GUIDE.md`; `README.md` completo com badges,
pré-requisitos, build nativo + cross e KMSDRM; `CHANGELOG.md` 0.1.0 e
`CONTRIBUTING.md` revisado; `cargo doc` limpo. Aceite: CI verde (ou validação local
documentada) e README cobrindo build/execução em x86_64 e ARM64.

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
- Próximas fases: 6 (CI/docs), 7 (capi com cbindgen + testes C/Python), 8 (binding
  Lua com `mini_game.lua` e subcomando `giochi-demo lua`).
