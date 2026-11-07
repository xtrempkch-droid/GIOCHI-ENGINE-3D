# Estado — GIOCHI-ENGINE

## Última fase concluída: 8 — Binding Lua (`giochi-engine-lua`)
Data: 2026-11-07

**Todas as fases 0–8 estão concluídas.**

## Gates validados

Comando único: `scripts/gates.sh` (`SKIP_ARM64=1` pula o cross-check).

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --target x86_64-unknown-linux-gnu`
- [x] `cargo test --workspace --target x86_64-unknown-linux-gnu` — 209 testes, 0 falhas
- [x] `cargo check --workspace --target aarch64-unknown-linux-gnu`
- [x] `cargo doc --workspace --no-deps --document-private-items` (0 warnings)
- [x] `scripts/gen-header.sh --check` — `giochi.h` em sincronia com o crate
- [x] `scripts/ffi-smoke.sh` — consumidores C e Python (fase 7, mantidos verdes)

Verificações manuais da fase:

- `giochi-demo lua .../mini_game.lua --auto --frames 600`: 600 frames, 59 fps,
  **pontuação final 7**, 3 vidas, `texturas vivas: 0`.
- `... --auto --ram --lives 1 --spawn 0.3 --speed 400 --frames 300`: **fim de jogo**
  aos 68 frames, 0 vidas — o caminho de colisão e game over funciona e pára o loop.
- `... --frames 60` sem `--auto`: 0 pontos, 3 vidas, sem fugas.
- `tests/lua_smoke.rs`: percorre toda a superfície (capacidades, texturas, `destroy`,
  recolha pelo GC, `giochi.quit()`, propagação de erro, `app:step`, `arg`).
- `tests/mini_game_script.rs`: o jogo de referência pontua, termina em game over e não
  deixa texturas vivas.

## Entregue na fase 8

- `giochi-engine-lua/src/` em seis módulos: `lib` (módulo `giochi`, `run_file`,
  mapeamento de erros), `app` (`App` + `LuaScene` + flag de quit), `caps`
  (`Gfx`/`InputRef`/`AudioRef`), `handles` (`Texture`/`Sound`/`Music`), `values`
  (conversão de cores e rectângulos) e `keys` (tabelas de teclas/ratos/eixos/botões).
  `#![forbid(unsafe_code)]`.
- Superfície Lua: `giochi.app`, `giochi.quit`, `giochi.keys/mouse/axes/buttons`,
  `app:texture/sound/music/set_volume/set_clear_color/run/step` e as estatísticas;
  `input:*` (teclado, rato, roda, gamepad), `audio:*` (sons, música, volume) e
  `gfx:*` (clear, rect, draw, sprite, camera, screen, texture, size).
- Capacidades por callback (ADR-030): o script só vê o que a engine lhe empresta, e as
  userdata são invalidadas pelo `mlua` quando o callback regressa.
- `Texture` libertado por `Drop` **e** por `:destroy()` (ADR-031); `Sound`/`Music`
  geridos pela app.
- `giochi-demo lua <script> [args...]`, com `arg[0]`/`arg[1..]` na convenção Lua.
- `giochi-engine-lua/examples/mini_game.lua`: space shooter completo, com beep
  sintetizado em WAV temporário (nenhum binário no repositório), flags `--auto`,
  `--frames`, `--lives`, `--ram`, `--spawn`, `--speed`, e uma tabela de resumo
  devolvida ao host.
- `docs/LUA_BINDING.md`, `docs/DECISIONS.md` (ADR-030, ADR-031),
  `docs/ARCHITECTURE.md` §5c, `README.md` com secção "From Lua", `CHANGELOG.md`.
- `ci.yml` corre `mini_game.lua` no job de smoke; `cross-compile.yml` faz o link ARM64
  real; `release.yml` empacota os dois alvos com a ABI C e o `mini_game.lua`.
- Testes: `giochi-engine-lua/tests/lua_smoke.rs` e `tests/mini_game_script.rs`.

## Auditoria final do prompt mestre

Depois da fase 8, o prompt foi reauditado item a item contra o repositório. Astrês
entregas em falta encontradas **e corrigidas** na mesma alteração:

- `FrameClock` era `pub` sem `#[repr(C)]` e sem `_reserved` — violava R1 e R3.
- `SceneManager` tinha `_reserved` mas não `#[repr(C)]` — violava R1.
- `giochi-engine/src/lib.rs` anunciava no doc de topo que as funções ainda faziam
  `todo!("fase N")`, o que deixou de ser verdade desde a fase 5.

E uma lacuna de processo: as regras R1/R3 eram verificadas por revisão. Agora
`giochi-engine/tests/c_shape.rs` verifica-as mecanicamente sobre todo o código-fonte
(ADR-032), incluindo a excepção única e deliberada (`GiochiError`, exigida pela R4).
Verificado por violação provocada: retirar `#[repr(C)]` a `Sprite` e encolher
`RunStats::_reserved` para `[u32; 2]` falham o gate com ficheiro, linha e regra.

Confirmado presente e conforme: os 70 ficheiros da estrutura do prompt (workspace,
8 docs, 3 workflows, árvore `src/`, `cbindgen.toml`, `giochi.h`, os três testes da
capi, `mini_game.lua`); `MAX_FRAME_TIME = 0.25` e `FIXED_TIMESTEP = 1/60`;
`init_sdl()` com os seis hints de KMSDRM documentados; `HandleRegistry` com
`HashMap<u64, Rc<T>>` e contador `AtomicU64`; zero `unwrap()`/`expect()` em código de
biblioteca (só em `#[cfg(test)]`); zero `todo!()`; zero `unsafe` fora da capi, onde
cada bloco tem `// SAFETY:`; nenhuma `pub fn` com generics ou lifetime explícito;
nenhum campo público com `Rc`/`Arc`/`Box`/`String`/`Vec`/`HashMap`.

## Saúde do repositório (verificação de 2026-11-07)

O CI no GitHub **falhava em todos os jobs** desde a fase 6. Causas encontradas e
corrigidas:

1. **Faltavam os headers X11** (`libxext-dev` e o resto do conjunto). O SDL2 `bundled`
   configura o backend X11 em qualquer runner que tenha bibliotecas X11 e falha com
   *"Missing Xext.h, maybe you need to install the libxext-dev"*. Passava localmente
   porque esta máquina tem os headers.
2. **Faltavam os satélites SDL2 no host** (`libsdl2-image/mixer/ttf-dev`): o `bundled`
   só compila o SDL2 core, e o link da `cdylib` falha com `cannot find -lSDL2_mixer`.
   Já estava documentado para ARM64; nunca tinha sido testado em x86_64.
3. **Lista de pacotes duplicada em quatro jobs** — a razão de 1 e 2 passarem
   despercebidos. Substituída por `scripts/install-deps.sh`, uma só lista.
4. **Três workflows tinham sido substituídos por versões reduzidas**, e os commits
   `feat(phase-7)`/`feat(phase-8)` (`git add -A`) levaram-nas consigo: `ci.yml` sem os
   jobs `ffi`/`smoke`/cross-check. Os três foram restaurados e passaram a chamar os
   scripts em vez de duplicarem comandos.
5. **Teste dependente do ambiente**: `api_guide.rs` exigia `INVALID_HANDLE` de
   `play_sound(0, 0)`, mas sem placa de som o mixer devolve `AUDIO` (ADR-021). Passava
   numa máquina com áudio, falhava num runner sem ele.

Também corrigido: `rust-version` declarava **1.75**, que nenhuma combinação das
dependências fixadas satisfaz (`mlua 0.10` exige 1.79; `size_of` no prelude exige 1.80).
Sondado com `cargo +1.79` (falha) e `cargo +1.82` (passa): o piso declarado é agora
**1.82**, com o mesmo número em `clippy.toml` e no README. ADR-033. Efeito imediato: o
clippy passou a sugerir `Result::inspect` (estável desde 1.76) em `read_asset_vec`, que
estava escrito à moda antiga por causa do piso falso.

Depois do primeiro envio, o CI real ficou verde em `ci.yml` (4 jobs) e expôs uma última
armadilha em `cross-compile.yml`: o `ubuntu-latest` já não é 24.04 mas 26.04, e o recipe
multiarch preso a `noble` colide em `libcrypt-dev` (amd64 *Breaks* arm64). Extraído para
`scripts/install-arm64-deps.sh`, que deriva a suite do `/etc/os-release`, com os dois jobs
de índices coincidentes fixados em `ubuntu-24.04`.

**Estado no GitHub (execuções reais, commit `79e8106`):**

| Workflow | Jobs | Resultado |
|---|---|---|
| `ci.yml` | gates, ffi, cross-check, smoke | ✅ verde |
| `cross-compile.yml` | check, link (`ubuntu-24.04`) | ✅ verde, link ARM64 real |
| `release.yml` | build-x86_64, build-aarch64, publish | ✅ verde, rascunho `v0.1.0` com os dois arquivos |

Validação: a matriz completa de CI espelhada num contentor `ubuntu:24.04` limpo — fmt,
clippy, build, 209 testes, doc, header, FFI (C + Python), demo, os cinco exemplos e o
`mini_game.lua` — **16/16 passos**, 0 falhas.

## Roadmap

As oito fases do prompt mestre estão completas. Trabalho futuro, por ordem de valor:

1. **Texto e fontes** — `GiochiFont` existe na ABI v1 apenas como `typedef` reservado;
   TTF já está na árvore via `sdl2` feature `ttf`. Seria a próxima adição natural (HUD
   legível em vez de barras).
2. **Pilha de cenas na ABI** — hoje `giochi_engine_run_scene` deixa a pilha vazia no
   fim; `push`/`pop` (menus, pausa) exigiriam `giochi_scene_push`/`pop` e uma regra
   para `on_enter`/`on_exit` com mais de uma cena viva.
3. **`giochi-engine-3d`** — crate separada em EGL + GLES, nunca um modo deste renderer
   (ADR-005).
4. **Empacotamento** — receita de deploy para consola (rootfs mínimo com
   `libdrm`/`libgbm`/`libEGL` e os satélites SDL2), hoje descrita em `docs/KMSDRM.md`
   §5 mas não automatizada.

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
  linker no `.cargo/config.toml` **e** os satélites ARM64. Corrigido; o link é
  verificado pelo job `link` do `cross-compile.yml` e pelo `build-aarch64` da release.
- `giochi.h` é gerado pelo `cbindgen` 0.27: quem não o tiver instalado vê o gate do
  header ser saltado em `scripts/gates.sh`, mas o CI falha se estiver desatualizado.
- A ABI v1 não expõe pilha de cenas: `giochi_engine_run_scene` deixa a pilha vazia no
  fim, pelo que cada handle equivale a uma cena ativa. `push`/`pop`/`replace` ficam
  para uma revisão futura da ABI.
- Uma app por processo (o engine é singleton, ADR-012). A camada Lua deteta a segunda
  tentativa com uma mensagem clara em vez do erro do `EventPump` do SDL.
- Em Lua, `Sound`/`Music` vivem até a app os destruir ou até ao fim do processo: o
  mixer pertence ao engine e não é alcançável num finalizador do GC (ADR-031).

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
- A capi não conhece `sdl2`: fala apenas com `giochi-engine`, e é a única crate do
  workspace com `unsafe`, com `// SAFETY:` em cada bloco. O core e a camada Lua são
  `#![forbid(unsafe_code)]`.
- A camada Lua usa `mlua::Lua::scope` para emprestar os subsistemas (ADR-030): é a
  versão segura da mesma regra que a capi implementa com ponteiros (ADR-028).
