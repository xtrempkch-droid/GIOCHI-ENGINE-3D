# Estado — GIOCHI-ENGINE

## Última fase concluída: 4 — Assets + Audio
Data: 2026-11-07

## Gates validados

Comando único: `scripts/gates.sh` (`SKIP_ARM64=1` pula o cross-check).

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --target x86_64-unknown-linux-gnu`
- [x] `cargo test --workspace --target x86_64-unknown-linux-gnu` — 158 testes, 0 falhas
- [x] `cargo check --workspace --target aarch64-unknown-linux-gnu`
- [x] `cargo doc --workspace --no-deps --document-private-items`

Verificações manuais da fase:

- `cargo run -p giochi-engine --example audio_test -- --beep --frames 5` (x11):
  dispositivo de áudio **real** disponível, WAV procedural gerado em runtime,
  `bipe #1 (via --beep)` impresso, saída com 0 e `modo silencioso: false`.
- `tests/assets_audio_smoke.rs` (driver `dummy`): `player.png` lido para um buffer
  do chamador (magia PNG conferida), buffer curto retorna `ASSET` sem truncar,
  **2 loads do mesmo caminho = 1 textura no registry**, caminho diferente = 2ª
  entrada, `destroy_texture` por fora do cache faz o próximo load recriar (com
  handle novo), `clear_resource_cache` zera o registry. Com dispositivo presente:
  WAV real carrega/toca/destrói, double-destroy reportado, `load_music("")`
  retorna `INVALID_ARGUMENT`; sem dispositivo: tudo degrada para `AUDIO` sem
  panic.
- `scripts/gates.sh` verde.

## Entregue na fase 4

- `assets/loader.rs`: `asset_size` (rejeita caminho vazio, não-arquivo e > 64 MiB),
  `read_asset(path, &mut [u8])` que **falha** em vez de truncar quando o buffer é
  curto, `read_asset_vec`, `asset_name`, `MAX_ASSET_SIZE = 64 MiB` — 8 testes.
- `assets/manager.rs`: `ResourceManager` com cache `HashMap<String, TextureHandle>`
  em `RefCell`, `load_texture(&TextureRegistry, path)` (2 loads = 1 upload),
  `get_texture`, `is_texture_cached`, `cached_texture_count`,
  `live_texture_count`, `clear`, `read_asset`. Handles obsoletos (textura destruída
  por fora) são detetados e recarregados — 3 testes unitários + cobertura de cache
  no teste de integração.
- `audio/sound.rs`: `SoundHandle`, `MusicHandle`, `MusicState`,
  `LOOPS_INFINITE`, `MAX_VOLUME = 128`, `SAMPLE_RATE`, `CHUNK_SIZE`,
  `MIXING_CHANNELS`, `clamp_volume` — 3 testes, com asserções de constantes em
  tempo de compilação.
- `audio/mixer.rs`: `AudioSystem` sobre `sdl2::mixer` com
  `HandleRegistry<Chunk>`/`HandleRegistry<Music<'static>>`; `load_sound`,
  `destroy_sound`, `play_sound`, `stop_all_sounds`, `load_music`,
  `destroy_music` (para a música antes de liberar), `play_music`, `pause_music`,
  `resume_music`, `stop_music`, `set_volume`/`volume`, `is_music_playing`,
  `music_state`, `sound_count`, `music_count`, `has_sound`, `has_music`, `clear`.
  `AudioSystem::silent()` e modo silencioso quando não há dispositivo — 7 testes.
- `platform/sdl.rs`: subsistema de áudio opcional (`Platform::audio_subsystem()`);
  sem dispositivo gera aviso, não erro.
- `render/renderer.rs`: `RenderContext` agora possui o `ResourceManager` e expõe
  `load_texture` (com cache), `cached_texture`, `cached_texture_count`,
  `resources()`, `resources_mut()`, `clear_resource_cache()`.
- `examples/audio_test.rs`: WAV 16-bit estéreo **sintetizado em runtime**
  (frequência 660 Hz, envelope de 20 ms), ESPAÇO toca, setas mudam o volume, M
  silencia, `--beep` toca uma vez no arranque e `--frames N` limita a execução.
- `tests/assets_audio_smoke.rs`: integração headless de loader, cache e áudio.
- `docs/DECISIONS.md`: ADR-021 (modo silencioso), ADR-022 (cache por string exata +
  validação de handle obsoleto), ADR-023 (`Chunk`/`Music` nos handle registries).
- `docs/C_ABI_CONTRACT.md`: `giochi_audio_available`, semântica de `loops`,
  volume e liberação de música.

## Próxima fase: 5 — Scene + Engine (fachada)

`scene/scene.rs` (`trait Scene` com `on_enter`/`on_exit`/`update`/`render`),
`scene/state.rs` (`SceneManager` com `push`/`pop`/`replace`/`current_mut`),
`core/engine.rs` (`Engine::new(config)`, `engine.run(Box<dyn Scene>)`), `giochi-demo`
refatorado para usar `Engine` e exemplo `mini_game` (Pong ou space shooter) usando
todos os módulos. Aceite: `mini_game` jogável em x86_64; cross-check ARM64 passa.

## Bloqueios / pendências

- Nenhum bloqueio de gate.
- CMake 4.2.3 × `cmake_minimum_required(VERSION 3.0.x)` do SDL2 vendorizado:
  workaround em `.cargo/config.toml` (ADR-014).
- `sdl2` compila com `unsafe_textures` (ADR-017), sob o pressuposto de um único
  canvas por processo (ADR-018).
- O caminho "ao vivo" do gamepad não é exercitável sem hardware (ADR-020).
- Nesta máquina o `kmsdrm` não inicializa (sessão gráfica detém DRM master); é o
  caso coberto pelo fallback do ADR-015. O dispositivo de áudio **existe**, então
  o caminho real de mixer foi verificado; o modo silencioso é coberto por
  `AudioSystem::silent()` e pelo teste de integração.
- `ResourceManager` não tem API C-shaped de `i32` nesta fase: o requisito do prompt
  (`load_texture(handle_out, path) -> i32`) é satisfeito na capi (fase 7) sobre
  `ResourceManager::load_texture`, que é a forma idiomática em Rust. Registrado
  aqui para não parecer omissão.

## Notas de arquitetura

- `src/error.rs` não estava na estrutura original do prompt; módulo de topo para
  não violar as regras de dependência (ADR-011).
- `EngineConfig.title` é `[u8; 64]` (ADR-007); `KeyCode`/`MouseButton`/
  `GamepadAxis`/`GamepadButton` são `u32` com valores SDL (ADR-009); ABI v1
  singleton (ADR-012).
- Ordem obrigatória por frame: `poll_events()` → `input.update(&events)` →
  `game_loop.advance()` → `scene.update()` → `render` → `present()`.
- Fluxo de posse do render: `Window` → `into_render_context()` → `RenderContext`
  (canvas + `TextureRegistry` + `ResourceManager` + `Camera`) — ADR-018.
- `TextureRegistry::create_texture` e `RenderContext::create_texture` **ignoram** o
  cache (cada chamada faz upload); `RenderContext::load_texture` usa o cache. A
  distinção é intencional: a primeira é a primitiva, a segunda a política.
- `AssetSystem`/`AudioSystem` degradam em vez de impedir o arranque (ADR-015,
  ADR-021): o jogo continua sem som e os erros são explícitos.
- `core` não importa `sdl2`; `render` não conhece `scene`; `math` não depende de
  módulos internos; `handle_registry` é `pub(crate)`.
- Fase 6 é dona de `.github/workflows/*`, dos templates de issue/PR e do README
  completo; o diretório existe vazio de propósito.
