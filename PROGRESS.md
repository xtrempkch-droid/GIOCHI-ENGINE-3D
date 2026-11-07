# Estado — GIOCHI-ENGINE

## Última fase concluída: 2 — Render 2D (SDL2 render, C-shaped)
Data: 2026-11-07

## Gates validados

Comando único: `scripts/gates.sh` (`SKIP_ARM64=1` pula o cross-check).

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --target x86_64-unknown-linux-gnu`
- [x] `cargo test --workspace --target x86_64-unknown-linux-gnu` — 100 testes, 0 falhas
- [x] `cargo check --workspace --target aarch64-unknown-linux-gnu`
- [x] `cargo doc --workspace --no-deps --document-private-items`

Verificações manuais da fase:

- `cargo run -p giochi-engine --example hello_rect -- --frames 30` (x11): abre
  960x540, desenha 3 retângulos, encerra com 0.
- `cargo run -p giochi-engine --example moving_sprite -- --frames 60` (x11 e
  wayland): carrega `player.png` (32x32) e `checker.png` (64x64), 2 texturas vivas,
  desenha 18 tiles + sprite via câmera, destrói ambas e termina com
  `restantes: 0`.
- `tests/render_smoke.rs` (headless, driver `dummy`): verifica **pixels** via
  `read_pixels` — o retângulo desenhado aparece em (20,20) e o fundo permanece em
  (48,48); handle 0 retorna `INVALID_HANDLE`; arquivo inexistente retorna `ASSET`;
  `destroy_texture` duplo retorna `false`; contador volta a 0.

## Entregue na fase 2

- `render/camera.rs`: `world_to_screen`, `screen_to_world` (inversa exata),
  `world_rect_to_screen`, `viewport_center`, `effective_zoom` que saneia
  `NaN`/infinito/`<= 0` para `1.0` — 9 testes.
- `render/texture.rs`: `TextureRegistry` com `HandleRegistry<Texture>`,
  `create_texture(path)`, `create_texture_from_rgba(w, h, &[u8])`,
  `destroy_texture`, `dimensions`/`size`, `contains`, `len`, `clear`; `Drop` limpa o
  registro; `ensure_image_subsystem` inicializa SDL_image uma vez por processo.
  `TextureSize` C-shaped.
- `render/renderer.rs`: `RenderContext` dono de canvas + registry + câmera;
  `clear`, `draw_rect`, `draw_texture(handle, dst, src)`, `draw_sprite`,
  `present`, `read_pixels`, `size`/`drawable_size`/`set_title`,
  `set_camera`/`camera`, `fit_camera_to_window` e delegações
  `create_texture`/`destroy_texture`/`texture_size`/`texture_count`.
  Conversão float→`SDL_Rect` com arredondamento, clamp de `i32` e no-op para área
  zero — 5 testes.
- `render/sprite.rs`: `world_rect`, `translate`, `is_drawable` (layout de 56 bytes
  preservado: nenhum campo novo).
- `math/vec2.rs` e `math/rect.rs`: operadores `Add`/`Sub`/`Mul` em `Vec2`,
  `length`/`distance`/`is_finite`; `Rect::{intersection, contains, center}` — 17
  testes de math.
- `handle_registry.rs`: `#![allow(dead_code)]` temporário removido (agora há
  consumidor real); `Window::sdl_window` substituído por `into_parts`.
- Exemplos `hello_rect` e `moving_sprite` (+ `--frames N`).
- `assets/player.png` (32x32, RGBA com transparência) e `assets/checker.png`
  (64x64) gerados sem dependências externas.
- `tests/render_smoke.rs`: integração headless com verificação de pixels.
- `docs/DECISIONS.md`: ADR-017 (`unsafe_textures` no `sdl2`) e ADR-018
  (`RenderContext` dono da janela e do registro).
- `docs/C_ABI_CONTRACT.md`: regras de arredondamento/`zoom`/handles inválidos,
  semântica de double-destroy e nota sobre o cache de texturas.

## Próxima fase: 3 — Input

`input/keyboard.rs` (`is_down`, `is_pressed_this_frame`, `is_released_this_frame`),
`input/mouse.rs` (posição flutuante, botões, wheel), `input/gamepad.rs` (primeiro
controle via `sdl2::controller`), `input/mod.rs` com `Input::update(&mut self,
events: &[Event])` chamado antes de `game.update`, e exemplo `input_test`
(retângulo WASD, muda cor com ESPAÇO, segue o mouse com botão direito). Aceite:
exemplo funcional e testes cobrindo as transições de estado.

## Bloqueios / pendências

- Nenhum bloqueio de gate.
- CMake 4.2.3 × `cmake_minimum_required(VERSION 3.0.x)` do SDL2 vendorizado:
  workaround em `.cargo/config.toml` (ADR-014).
- `sdl2` passou a ser compilado com `unsafe_textures` (ADR-017). É o que permite
  guardar `Texture` num `HashMap<u64, Rc<Texture>>` sem lifetime público; o
  pressuposto é **um único canvas por processo**, garantido pelo ADR-018.
- `read_pixels` força cópia GPU→CPU: existe para testes e screenshots, nunca no
  frame loop (documentado no método).
- `TextureRegistry::create_texture` (fase 2) ainda faz upload por chamada; o cache
  por caminho chega com `ResourceManager` na fase 4.
- Nesta máquina o `kmsdrm` não inicializa (sessão gráfica detém DRM master); é o
  caso coberto pelo fallback do ADR-015.

## Notas de arquitetura

- `src/error.rs` não estava na estrutura original do prompt; módulo de topo para
  não violar as regras de dependência (ADR-011).
- `EngineConfig.title` é `[u8; 64]` (ADR-007); `KeyCode = u32` (ADR-009); ABI v1
  singleton (ADR-012).
- Fluxo de posse do render: `Window` → `into_render_context()` → `RenderContext`
  (canvas + `TextureRegistry` + `Camera`). O `Window` é consumido porque o SDL
  vincula o renderer à janela (ADR-018).
- `RenderContext::textures()` empresta `&self`; em loop de desenho use as
  delegações (`create_texture`, `destroy_texture`, `texture_size`) para não
  conflitar com `&mut self`.
- `render` não conhece `scene`; `core` não importa `sdl2`; `math` não depende de
  módulos internos (só de `crate::math`).
- Fase 6 é dona de `.github/workflows/*`, dos templates de issue/PR e do README
  completo; o diretório existe vazio de propósito.
