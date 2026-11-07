# Estado — GIOCHI-ENGINE

## Última fase concluída: 3 — Input
Data: 2026-11-07

## Gates validados

Comando único: `scripts/gates.sh` (`SKIP_ARM64=1` pula o cross-check).

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --target x86_64-unknown-linux-gnu`
- [x] `cargo test --workspace --target x86_64-unknown-linux-gnu` — 136 testes, 0 falhas
- [x] `cargo check --workspace --target aarch64-unknown-linux-gnu`
- [x] `cargo doc --workspace --no-deps --document-private-items`

Verificações manuais da fase:

- `cargo run -p giochi-engine --example input_test -- --frames 60` (x11): abre
  960x540, roda 58 passos fixos, imprime `gamepad: nenhum`, encerra com 0.
- `tests/input_smoke.rs` (driver `dummy`): injeta eventos **SDL reais** via
  `Platform::push_event` e verifica o caminho completo evento → `Input::update` →
  consulta: `W` fica `down` e `pressed` por um frame; a borda limpa no frame
  seguinte; botão direito chega com a posição (40,30); `Escape` também para o
  `GameLoop` via `quit_signal`.
- `platform.game_controller_subsystem()` responde `Ok` com o driver `dummy` e
  `gamepad()` reporta desconectado, sem falhar.

## Entregue na fase 3

- `input/keyboard.rs`: `KeyCode = u32` (+ constantes `KEY_*`), `KeyboardState` com
  `is_down`, `is_pressed_this_frame`, `is_released_this_frame`, `press`, `release`,
  `begin_frame`, `reset`, `down_count`, `any_down` — 10 testes.
  Regra de "tap": pressão e soltura no mesmo frame expõem **as duas** bordas;
  auto-repeat do SDL não re-dispara `pressed`.
- `input/mouse.rs`: `MouseButton = u32` (+ `MOUSE_*`), `MouseState` com posição
  float, `delta` acumulado por frame, botões com as três consultas, `wheel_delta`
  acumulado e resetado, `set_position`, `add_delta`, `add_wheel` — 8 testes.
- `input/gamepad.rs`: `GamepadState` sobre `sdl2::controller::GameController`
  ("primeiro controle"), `is_connected`, `name`, `instance_id`, `axis` normalizado
  e clampado em `[-1,1]`, `is_down`, `open_first` (índices 0..8), `open_index`,
  `close`, `close_if_instance`; mapeamento `u32` ↔ `SDL_CONTROLLER_*` — 4 testes.
- `input/mod.rs`: `Input` agregando os três estados, `Input::new(&Platform)`,
  `Input::without_gamepad()`, `update(&[Event])` (limpa bordas → aplica eventos →
  abre controle), `begin_frame`, `apply_event`, acessores read-only e `*_mut` para
  injeção, atalhos `key_down`/`key_pressed`/`key_released`/`mouse_down` — 13 testes.
- `platform/sdl.rs`: subsistema de game controller inicializado de forma opcional
  (aviso, não erro) e exposto por `Platform::game_controller_subsystem()`.
- `examples/input_test.rs`: quadrado com WASD/setas, ESPAÇO troca a cor, botão
  direito segue o mouse, botão esquerdo marca cliques que desaparecem, roda
  escala, analógico/D-pad do controle move; `--frames N`.
- `tests/input_smoke.rs`: integração headless com eventos SDL reais.
- `docs/C_ABI_CONTRACT.md`: tabela de semântica de input (bordas, tap, repeat,
  ids desconhecidos, delta, wheel, normalização de eixos, seleção de controle).
- `docs/DECISIONS.md`: ADR-019 (`Input` como snapshot por frame, com estado
  injetável) e ADR-020 (eixos/botões de gamepad consultados ao vivo).

## Próxima fase: 4 — Assets + Audio

`assets/loader.rs` (`asset_size`, `read_asset` com buffer do chamador e limite de
64 MiB), `assets/manager.rs` (`ResourceManager` com cache por caminho: 2 loads = 1
upload real), `audio/mixer.rs` (`AudioSystem` sobre `sdl2::mixer`), `audio/sound.rs`
(`SoundHandle`/`MusicHandle`, `play_sound`, `play_music`, `pause`, `resume`, `stop`,
`set_volume`, `is_music_playing`) e exemplo `audio_test` (WAV procedural tocado com
ESPAÇO). Aceite: `audio_test` toca áudio (ou degrada com erro claro quando não há
dispositivo, documentado em `PROGRESS.md`) e `ResourceManager` testado com
2 loads = 1 real.

## Bloqueios / pendências

- Nenhum bloqueio de gate.
- CMake 4.2.3 × `cmake_minimum_required(VERSION 3.0.x)` do SDL2 vendorizado:
  workaround em `.cargo/config.toml` (ADR-014).
- `sdl2` compila com `unsafe_textures` (ADR-017), sob o pressuposto de um único
  canvas por processo (ADR-018).
- O caminho "ao vivo" do gamepad (eixos/botões) não é exercitável sem hardware
  real nem sem um controle virtual: os testes cobrem mapeamento de ids,
  normalização e o estado "desconectado ⇒ neutro" (ADR-020). `open_first` com
  driver `dummy` retorna `false` sem erro.
- Nesta máquina o `kmsdrm` não inicializa (sessão gráfica detém DRM master); é o
  caso coberto pelo fallback do ADR-015.

## Notas de arquitetura

- `src/error.rs` não estava na estrutura original do prompt; módulo de topo para
  não violar as regras de dependência (ADR-011).
- `EngineConfig.title` é `[u8; 64]` (ADR-007); `KeyCode`/`MouseButton`/`GamepadAxis`
  /`GamepadButton` são `u32` com valores SDL (ADR-009); ABI v1 singleton (ADR-012).
- Fluxo de posse do render: `Window` → `into_render_context()` → `RenderContext`
  (canvas + `TextureRegistry` + `Camera`) — ADR-018.
- Ordem obrigatória por frame: `poll_events()` → `input.update(&events)` →
  `game_loop.advance()` → `scene.update()` → `render` → `present()`. As bordas
  (`pressed`/`released`) só são válidas entre `update` e o próximo `update`.
- `Input` guarda `Option<GameControllerSubsystem>` clonado do `Platform`; se o
  subsistema não existe, o gamepad nunca reporta conexão e o resto do input segue.
- `core` não importa `sdl2`; `render` não conhece `scene`; `math` não depende de
  módulos internos; `handle_registry` é `pub(crate)`.
- Fase 6 é dona de `.github/workflows/*`, dos templates de issue/PR e do README
  completo; o diretório existe vazio de propósito.
