# Estado — GIOCHI-ENGINE

## Última fase concluída: 6 — Polimento + CI + Docs
Data: 2026-11-07

## Gates validados

Comando único: `scripts/gates.sh` (`SKIP_ARM64=1` pula o cross-check).

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --target x86_64-unknown-linux-gnu`
- [x] `cargo test --workspace --target x86_64-unknown-linux-gnu` — 183 testes, 0 falhas
- [x] `cargo check --workspace --target aarch64-unknown-linux-gnu`
- [x] `cargo doc --workspace --no-deps --document-private-items` (0 warnings)

Verificações manuais da fase:

- `scripts/gates.sh` completo: exit 0, "All gates passed.".
- Smoke headless (`SDL_VIDEODRIVER=dummy`), os mesmos comandos que o CI roda:
  `giochi-demo --verify` (1 frame, `texturas vivas: 1, sons vivos: 1, áudio:
  ativo`), `giochi-demo --frames 60` (61.8 fps), `hello_rect`, `moving_sprite`,
  `input_test` e `audio_test` com `--frames 30`, `mini_game --auto --frames 120`
  (pontuação 10).
- Release x86_64: `cargo build --release --workspace` (LTO + codegen-units=1) e
  `giochi-demo --verify` a partir do binário release — usado pelo `release.yml`.
- Cross ARM64 **com link** verificado em container limpo `ubuntu:24.04`: a receita
  multiarch documentada em `docs/CROSS_COMPILE.md` produz
  `ELF 64-bit LSB pie executable, ARM aarch64`. Ver ADR-027.
- YAML dos três workflows e do `dependabot.yml` validado com um parser real.

## Entregue na fase 6

- `docs/ARCHITECTURE.md`: diagrama de camadas, tabela das regras de dependência,
  propriedade e lifetimes, ordem obrigatória por frame, tabela dos mecanismos que
  impõem a superfície C-shaped, política de falha, estratégia de testes e
  perspectivas das fases 7/8.
- `docs/KMSDRM.md`: negociação de driver, variáveis de ambiente, permissões em
  `/dev/dri` (grupos `video`/`render`), tabela das mensagens de falha típicas,
  bibliotecas de runtime necessárias no alvo (incluindo os satélites SDL2 e o
  fecho de dependências) e receita de verificação.
- `docs/API_GUIDE.md`: guia orientado a tarefas — exemplo de 5 minutos,
  configuração, cenas e transições, inspeção do loop, screen-space vs world-space,
  semântica de bordas de input, áudio e modo silencioso, assets, matemática,
  tabela completa de códigos de erro e padrões recomendados.
- `giochi-engine/tests/api_guide.rs`: compila e executa **todos** os exemplos do
  guia, para que a documentação não divirja do código.
- `docs/CROSS_COMPILE.md` reescrito: os dois scripts, a receita multiarch
  verificada, sysroot de console, requisito de linker, satélites SDL2 e o caminho
  `cross`.
- `.github/workflows/ci.yml`: gates em x86_64, cross-check ARM64 e smoke do demo e
  dos cinco exemplos.
- `.github/workflows/cross-compile.yml`: job `check` (rápido) e job `link` (receita
  multiarch + `scripts/build-aarch64.sh --workspace --all-targets`).
- `.github/workflows/release.yml`: binários x86_64 e aarch64, smoke do binário de
  release, empacotamento em `.tar.gz` e anexo a um release em rascunho.
- `.github/ISSUE_TEMPLATE/{bug_report,feature_request}.md`,
  `.github/PULL_REQUEST_TEMPLATE.md`, `.github/dependabot.yml`.
- `scripts/build-aarch64.sh`: build ARM64 **com link**, resolve os satélites a
  partir de `CROSS_SYSROOT` ou do diretório multiarch, falha com instruções
  acionáveis quando faltam e afirma que os binários são ELF AArch64.
- `.cargo/config.toml`: `linker = "aarch64-linux-gnu-gcc"` para o alvo ARM64
  (bug real, ver ADR-027).
- `README.md` reescrito para 0.1.0: badges, pré-requisitos, build nativo e cross,
  troca de driver KMSDRM, exemplos e links para todos os guias.
- `CHANGELOG.md`: seção `## [0.1.0] - 2026-11-07` com todas as fases e
  `[Unreleased]` vazio para a próxima.
- `CONTRIBUTING.md`: referência a `scripts/gates.sh`, ao `api_guide.rs` como teste
  de documentação e tabela de "que documento atualizar quando".
- `docs/DECISIONS.md`: ADR-027.

## Próxima fase: 7 — C ABI (`giochi-engine-capi`)

Superfície `#[no_mangle] extern "C"` sobre `giochi-engine`: `giochi_get_last_error()`
(thread-local), `giochi_engine_version()`, funções `giochi_*` da tabela do
`docs/C_ABI_CONTRACT.md`, `cbindgen.toml` + `include/giochi.h` gerado,
`tests/rust_ffi.rs`, `tests/c_smoke.c` e `tests/python_smoke.py`, e `docs/C_ABI.md`.
Aceite: um programa C e um programa Python carregam a `cdylib`/`staticlib`, chamam
a engine e leem a mensagem de erro pelo código de retorno.

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
- Próximas fases: 7 (capi com cbindgen + testes C/Python), 8 (binding Lua com
  `mini_game.lua` e subcomando `giochi-demo lua`).
