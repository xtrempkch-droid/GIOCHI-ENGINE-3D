# Estado — GIOCHI-ENGINE

## Última entrega: fase 15 — Profiler + Debug Overlay (extensão de performance)
Data: 2026-11-08

## Última fase concluída: 15 — Profiler + Debug Overlay
Fase anterior: 14 — C ABI 3D (v2)

**Fases concluídas: 0–8 (2D), 9–14 (extensão 3D) e 15 (performance). Versão 0.2.2.**

## Status 2D (Fases 0–8)

Todas concluídas — nenhuma regressão tolerada pelas fases 3D (regra de ouro da
extensão v2.1). Gates 2D revalidados a cada fase 3D.

## Status 3D (Fases 9–14)

- [x] Fase 9 — Bootstrap 3D: contexto GLES/EGL, clear, swap
- [x] Fase 10 — Math 3D (`Vec3`, `Quat`, `Mat4`, `Transform3D`, `Camera3D`)
- [x] Fase 11 — Pipeline de mesh + shaders
- [x] Fase 12 — Câmera com controlo + `mini_fps`
- [x] Fase 13 — Iluminação Phong
- [x] Fase 14 — C ABI 3D (v2)

## Status Performance (Fases 15–18)

- [x] Fase 15 — Profiler + Overlay (baseline x86_64: **645.5 FPS @ 1000 sprites**,
      1000 draw calls; ARM64 PX30: **59.9 FPS** limitado pelo painel, **1.761 ms** de
      `render2d` a 1000 sprites; custo do profiler ≤ **1.1 %** x86_64 e **1.94 %** no
      PX30 com a overlay visível, dentro do orçamento de 2 %)
- [x] Fase 16 — Batching 2D + Atlas (1000 sprites → **1 draw call** em todos os
      backends; `opengl` 896.9 → **4024.8 FPS** com a câmara corrigida e
      `render2d` 0.403 → **0.131 ms** na cena da fase 15; PX30 `render2d` a 20000
      sprites 28.495 → **22.853 ms**, e 10 texturas intercaladas vs atlas a 5000
      sprites 10.8 → **58.6 FPS**)
- [x] Fase 17 — Instancing 3D + Static Batching + Upscaling (mini_fps 100 cubos:
      **103 → 4 draw calls**, campo 100 → **1**; x86_64 **5.87x** de FPS e
      **12.19x** no benchmark a 100 cubos; PX30 segura 60 FPS com 2000 cubos onde o
      loop faz 21.9; upscaling a 0.75 no PX30 **+61.3 %**)
- [x] Fase 18 — Flags ARM64 + CI de performance + C ABI v3 (binário **−24.1 %** no
      x86_64 e **−31.8 %** no ARM64 com velocidade inalterada — medido, não suposto;
      `cortex-a35 +crc,+aes,+sha2` provado na linha de comando do compilador e sem SIGILL
      no PX30; gate de regressão com **0 alocações por frame** e
      `ceil(N/1024)` draw calls; `GIOCHI_ABI_VERSION 3` aditivo, com os quatro
      consumidores C/Python das versões 1 e 2 passando sem alteração)

**Próxima fase: nenhuma planeada.** A extensão de performance (fases 15–18) está
concluída. O que fica aberto, e é honesto dizê-lo:

- `GIOCHI_GL_VERSION=3` nunca foi testado no PX30 — a fase 17 mediu tudo em GLES 2.0 com
  `GL_EXT_instanced_arrays`, que é o que o driver oferece por omissão.
- O bug de overlay em `mini_fps` (o renderer 3D não tem caminho 2D para desenhar texto)
  continua aberto.
- A comparação de benchmarks no CI corre contra `main` no mesmo runner, mas nunca foi
  observada a falhar num PR real: a primeira vez que ela reprovar algo é a primeira vez
  que ela prova o que vale.


## Gates validados

Comando único: `scripts/gates.sh` (`SKIP_ARM64=1` pula o cross-check).

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --target x86_64-unknown-linux-gnu`
- [x] `cargo test --workspace --target x86_64-unknown-linux-gnu` — 477 testes, 0 falhas
- [x] `cargo check --workspace --target aarch64-unknown-linux-gnu`
- [x] `cargo doc --workspace --no-deps --document-private-items` (um warning pré-existente em `balance.rs`)
- [x] `scripts/gen-header.sh --check` — `giochi.h` em sincronia com o crate
- [x] `scripts/ffi-smoke.sh` — **quatro** consumidores reais: `c_smoke.c`, `python_smoke.py`,
      `c_smoke_3d.c` e `python_smoke_3d.py`, todos verdes contra o `libgiochi.so` e o
      `giochi.h` gerados (`abi 2, engine 0.2.0`)
- [x] `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test context_smoke`
      — 3 testes contra GLES real (Mesa/EGL), incluindo leitura de pixel; 4 com
      `--features profiling` (contadores de mesh)
- [x] `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test lighting`
      — 7 testes que leem o framebuffer e verificam a **sombra**, não a chamada

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

## Fase 15 — Profiler + Debug Overlay
Data: 2026-11-08

**Escopo:** medir antes de otimizar. Nenhuma otimização foi feita nesta fase; o
renderer 2D continua a emitir um draw call por sprite. O que fica é o instrumento, a
overlay e a baseline que a fase 16 tem de bater.

### Entregue

- `giochi-engine/src/perf/`:
  - `frame_stats.rs` — `FrameStats` (tempos em `u64` ns, pronto para a ABI v3),
    `FrameTimeSummary` (min/max/avg/p50/p99) e `FrameCounters` (draw calls, sprites,
    meshes, vértices, bytes), todos `#[repr(C)]` com `_reserved`.
  - `profiler.rs` — `Profiler` com `begin_frame`, `begin_scope("update" | "render2d" |
    "render3d" | "audio")`, `end_scope`, `add_audio_time`, `end_frame`, `snapshot` e
    `summary`; janela de 60 amostras em `Mutex<Vec<FrameStats>>`.
  - `text.rs` — fonte bitmap 5x7 escrita no repo; sem ficheiro TTF e sem dependência.
  - `overlay.rs` — `DebugOverlay` desenhado com `sdl2::render`: uma textura
    persistente atualizada no lugar a 1 Hz (1x em painéis < 800 px), `F1` alterna,
    mostra FPS, frame, p99, CPU, draw calls, sprites, meshes, vértices, tempos por
    sistema e RSS de `/proc/self/status`.
- Feature `profiling` no core (e encaminhada em `giochi-engine-3d` e `giochi-demo`).
  Sem a feature, todos os métodos do profiler são corpos vazios inlined, os renderers
  não incrementam contadores e a overlay retorna imediatamente.
- Integração no `Engine`: scopes `update` e `render2d`, contadores antes da overlay,
  painel desenhado depois de `end_frame` (para não se contar a si próprio), `F1`,
  `frame_stats()`, `frame_time_summary()` e `is/set/toggle_overlay_visible()`.
- Contadores no `RenderContext` (2D: draw calls e sprites, incluindo `draw_rect`,
  `fill_rects`, linhas e texturas) e no `RenderContext3D` (draw calls, meshes,
  vértices e bytes), com reset em `begin_frame`/`begin_frame_stats`.
- Tempo de áudio medido pelo próprio `AudioSystem` num `Cell<u64>` (o mixer é
  emprestado diretamente à cena, não há wrapper onde cronometrar).
- `giochi-demo/examples/profiler_demo.rs` — cena de N sprites com uma textura,
  `--sprites`, `--frames`, `--width`, `--height`, `--no-overlay`, parser que rejeita
  `--frames=3oo`,
  modo interativo com `F1` e modo benchmark com uma linha legível por máquina.
  Fica em `giochi-demo` porque o comando de aceitação é
  `cargo run -p giochi-demo --example profiler_demo --features profiling` (a
  estrutura da extensão listava-o em `giochi-engine/examples`).
- `docs/PERF_LOG.md` com hardware, método, baseline e o teste de overhead.
- ADR-042 em `docs/DECISIONS.md` — overlay manual com fonte bitmap em vez de
  egui/puffin, e tempos em nanossegundos em vez de `Duration`.

### Baseline x86_64 (release, vsync off, 1280x720, mediana de 5×600 frames)

| Sprites (1 textura) | FPS | Frame avg | Frame p99 | CPU avg | Render2D avg | Draw calls |
|---|---:|---:|---:|---:|---:|---:|
| 200 | 1889.6 | 0.526 ms | 0.636 ms | 0.039 ms | 0.027 ms | 200 |
| 500 | 1073.4 | 0.925 ms | 1.079 ms | 0.078 ms | 0.065 ms | 500 |
| 1000 | 645.5 | 1.546 ms | 1.686 ms | 0.142 ms | 0.128 ms | 1000 |

Custo da feature `profiling` ligada: **≤1.1 %** (649.9 → 642.8 FPS a 1000 sprites com
a overlay escondida; 645.5 com a overlay visível, dentro do ruído). O orçamento da
extensão é 2 %.

### Baseline ARM64 (PX30, KMSDRM a 640x480, mediana de 3×600 frames)

| Sprites (1 textura) | FPS | CPU avg | Render2D avg | Draw calls |
|---|---:|---:|---:|---:|
| 200 | 60.0 | 0.762 ms | 0.547 ms | 200 |
| 500 | 60.0 | 1.203 ms | 1.004 ms | 500 |
| 1000 | 59.9 | 1.964 ms | 1.761 ms | 1000 |
| 5000 | 59.8 | 6.267 ms | 5.909 ms | 5000 |
| 10000 | 59.6 | 8.712 ms | 8.178 ms | 10000 |
| 20000 | 35.7 | 18.243 ms | 16.610 ms | 20000 |

O painel KMSDRM limita a ~60 FPS até ~10000 sprites; a fase 16 tem de mostrar o ganho
em `render2d_avg` (1000 sprites) e/ou no cenário de 20000 sprites. Overhead no PX30 a
1000 sprites: **0.56 %** do tempo de parede com a overlay escondida e **1.94 %** com a
overlay visível, medido por CPU do processo (`/usr/bin/time`), dentro dos 2 %.

### Gates da fase

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo clippy -p giochi-engine -p giochi-engine-3d -p giochi-demo --all-targets --features profiling -- -D warnings`
- [x] `cargo build --workspace --target x86_64-unknown-linux-gnu`
- [x] `cargo build --release -p giochi-demo --example profiler_demo --features profiling`
- [x] `cargo test --workspace --target x86_64-unknown-linux-gnu` — 477 testes, 0 falhas
- [x] `cargo test -p giochi-engine --features profiling` — 190 unit + integração, 0 falhas
- [x] `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --features profiling`
      — 173 unit + integração, incluindo o contador de meshes, 0 falhas
- [x] `CROSS_SYSROOT=/tmp/r35s-sysroot scripts/build-aarch64.sh --release -p giochi-demo --example profiler_demo --features profiling`
      — binário AArch64 confirmado; corrido no PX30 (ver `docs/PERF_LOG.md`)
- [x] `cargo doc --workspace --no-deps --document-private-items` — sem warnings do
      código da fase; permanece um warning pré-existente em
      `giochi-engine-3d/src/game/balance.rs` (link para `Ball::step`, privado)
- [x] `cargo check --workspace --target aarch64-unknown-linux-gnu`
- [x] `scripts/gen-header.sh --check` — `giochi.h` inalterado; a ABI v3 só chega na fase 18c
- [x] `scripts/check-version.sh`
- [x] Smoke do demo: `profiler_demo --sprites 200 --frames 120` imprime
      `draw_calls=200` e `frame_p99` não nulo.
- [x] Dois pressupostos de teste pré-existentes corrigidos, porque os gates os
      expuseram: `mesh_pipeline::release_all_leaves_nothing_behind` ainda esperava dois
      built-in shaders depois de a fase 13 acrescentar `Phong` (passa a usar
      `BuiltinShader::ALL.len()`); e `api_guide`/`engine_smoke` contavam a textura
      persistente da overlay como fuga quando a feature `profiling` está ligada (os
      testes desligam a overlay antes de contar).

### Pendências

- **ARM64 medido no PX30** (ver `docs/PERF_LOG.md`). O aparelho estava acessível por
  SSH; o launcher foi desativado e restaurado com o procedimento do
  `scripts/hardware-check.sh`, e o binário correu de `/tmp` (RAM), sem escrever no
  rootfs. Fica registado que o KMSDRM limita a ~60 FPS até ~10000 sprites: o critério
  de FPS da fase 16 no aparelho tem de usar 20000 sprites ou `render2d_avg`.
- **Overhead no cenário de stress.** Com 20000 sprites, os contadores por draw call
  custam ~5 % de CPU e a overlay visível baixa o frame rate de 39.8 para 35.7 FPS. É
  o cenário que a fase 16 elimina ao agrupar os draws; o orçamento de 2 % é medido na
  baseline de 1000 sprites.
- A overlay não aparece em `mini_fps` (o renderer 3D não tem caminho 2D para desenhar
  texto). Os contadores 3D estão medidos e expostos; a overlay 3D fica para quando
  houver um caminho de UI GLES.

## Fase 18 — Flags ARM64 + Benchmark Suite + CI de Performance + C ABI v3
Data: 2026-11-08

**Escopo:** compilar para o CPU do console, medir o custo por frame, criar um gate que
não possa falhar por ruído, e expor as estatísticas de frame ao C. A API existente não
muda (R12); a ABI C cresce de forma aditiva.

### O que o plano pedia e o que o hardware permite

Duas sugestões do plano não eram utilizáveis como escritas:

- **`-C target-cpu=native`** não pode funcionar num cross build: o cargo compila no host,
  então "native" descreve a máquina de desenvolvimento. O núcleo tem de ser nomeado, e o
  do PX30 é **Cortex-A35** (`CPU part 0xd04`), não o `cortex-a53` que o plano sugeria.
- **`+lse`** foi rejeitado. `grep Features /proc/cpuinfo` no aparelho devolve
  `fp asimd evtstrm aes pmull sha1 sha2 crc32 cpuid`: há `crc32`, `aes` e `sha2`, mas
  **não há `atomics`**. LSE é ARMv8.1 e o A35 é ARMv8.0-A, portanto `+lse` emitiria
  instruções que o núcleo não decodifica — o primeiro `Arc::clone` do primeiro frame
  seria SIGILL. `+crc`, `+aes` e `+sha2` ficam; `+lse` não, e o motivo está escrito ao lado da
  flag nos dois lugares onde ela seria adicionada.

Houve uma segunda armadilha, mais silenciosa: `.cargo/config.toml` tinha as flags e o
build continuava a sair para ARMv8-A genérico, porque `scripts/build-aarch64.sh` exporta
`RUSTFLAGS` (para o caminho de link do sysroot) e o cargo **ignora completamente** o
`rustflags` do ficheiro de configuração quando essa variável existe. Nada falhava. O
script passou a acrescentar as flags ele próprio, e o facto foi verificado com um
`RUSTC_WRAPPER` que registra a linha de comando real do compilador.

`panic = "abort"` também foi rejeitado: `docs/C_ABI_CONTRACT.md` garante que nenhum pânico
Rust cruza a fronteira (`catch_unwind` → `GIOCHI_ERROR_INTERNAL`), e `abort` destruiria
essa garantia para poupar ~5 % de tamanho.

### Resultado medido

| Medida | x86_64 | ARM64 (PX30) |
|---|---|---|
| `mini_fps` | 2 073 496 → **1 574 288 B** (−24.1 %) | 2 024 560 → **1 381 360 B** (−31.8 %) |
| `profiler_demo` | 719 648 → **487 480 B** (−32.3 %) | — |
| `giochi-demo` | 2 909 960 → **2 300 368 B** (−20.9 %) | — |
| FPS, `--instanced` 100 cubos | 15 072 → 14 884 (mediana de 5, dentro do ruído) | — |
| Frame vazio (custo do engine) | 267.9 → 262.8 µs (−1.9 %, no ruído) | — |
| FPS, `--loop --cubes 2000` | — | 21.7 → 21.6 (inalterado) |

**A conclusão da fase é tamanho, não velocidade.** No ARM64 o `strip` sozinho vale
−25.3 % e o LTO/codegen os outros 8.7 % (`.text` 1 414 302 → 1 323 066 bytes). A
velocidade não mudou em nenhuma das duas máquinas, o que é o resultado esperado: o frame
do PX30 é limitado pelo driver a escoar draw calls, não pela qualidade do código inteiro,
e `+crc`/`+aes`/`+sha2` não têm onde render — o engine não faz CRC nem criptografia.

O `strip = "symbols"` foi verificado contra a ABI C em vez de assumido: o `.symtab`
desaparece mas o `.dynsym` fica, e as 74 funções `giochi_*` continuam resolúveis
(`nm -D`).

### O gate de regressão encontrou uma alocação real

`tests/perf_regression.rs` conta alocações com um alocador global embrulhado, e falhou na
primeira execução: um frame em regime com 512 sprites batched alocava **exatamente uma vez
por frame**. A causa era `SpriteBatch::flush_runs`, que devolvia `Vec<RunView>` — um
`collect()` a cada flush, desde a fase 16. Passou a devolver um iterador, e `flush_batch`
desestrutura `self` para que o iterador sobre `batch` viva enquanto o corpo empresta
`textures` e `canvas`. O frame vazio e o de 512 sprites estão agora em 0 alocações.

O custo em tempo dessa alocação nunca foi mensurável; fica registado como o que é: uma
melhoria encontrada por um gate, não uma vitória de performance.

### CI e ABI

- `.github/workflows/perf.yml`: job bloqueante com o gate determinístico (alocações e
  draw calls, com e sem a feature `profiling`) e um job que constrói e mede `main` e o
  branch **no mesmo runner, minutos um do outro**, comparando com
  `scripts/perf-compare.py` a 10 %. Medir contra números gravados noutra máquina é como
  a CI de benchmark ganha reputação de mentir.
- `GIOCHI_ABI_VERSION 2 → 3`: `giochi_engine_frame_stats` + `GiochiFrameStats` (120
  bytes, alinhamento 8). É uma **projeção** do `FrameStats` Rust, com um `From` explícito
  que falha a compilar quando o tipo Rust ganha um campo. Aditivo: `c_smoke.c` continua a
  testar `>= 1`, e `c_smoke_3d.c`/`python_smoke_3d.py` passaram de `== 2` para `>= 2` e
  nada mais — que é precisamente a prova de compatibilidade que a fase devia dar.

Ver `docs/PERF_LOG.md` (secção "After Phase 18"), ADR-045 e
`docs/CROSS_COMPILE.md` (tabela `GIOCHI_TARGET_CPU`).

## Fase 17 — Instancing 3D + Static Batching + Upscaling
Data: 2026-11-08

**Escopo:** reduzir o custo por objeto no caminho 3D e permitir baixar a resolução de
render. A API existente não muda (R12): tudo é aditivo.

### Entregue

- `giochi-engine-3d/src/context/instancing.rs` — deteção, uma vez na criação do
  contexto, de três fornecedores: GLES 3.0 core, `GL_ANGLE_instanced_arrays` e
  `GL_EXT_instanced_arrays`. Os dois últimos carregam as entry points por
  `SDL_GL_GetProcAddress`. **A extensão que o plano nomeou (ANGLE) não existe em
  nenhuma das duas máquinas** — ambas expõem `GL_EXT_instanced_arrays`; um detetor
  escrito à letra do plano teria caído no fallback em 100 % do hardware disponível.
- `GlContext` passou a guardar `GL_EXTENSIONS` (consultada na criação, porque a string
  só é válida com o contexto corrente) e o backend de instancing já carregado.
- `render3d/instanced.rs` — `InstancedRenderer`: VBO de matrizes por instância,
  reempacotadas a cada draw (o `Mat4` tem 80 bytes e o driver quer 64; um `cast` leria
  cada instância no offset errado e desenhava lixo em vez de falhar).
- `RenderContext3D::draw_instanced(mesh, shader, &[Mat4], camera, cor) -> Result3D<u32>`
  devolve o **número de draw calls emitidas** (1 com extensão, N sem, 0 vazio), para que
  o fallback seja medível em vez de inferido pelo FPS.
- `shaders/instanced.vert` + `BuiltinShader::Instanced`, com o **mesmo** `phong.frag`
  para que as duas pernas do benchmark desenhem a mesma imagem. A matriz vem em quatro
  `vec4` (slots 3..6), coluna-maior; o caminho instanciado não usa VAO, porque
  `glVertexAttribDivisor` é estado do VAO e contaminaria o próximo draw normal da mesh.
- `mesh/static_batch.rs` — `StaticBatchBuilder`/`StaticBatch`: transforma os vértices no
  CPU, concatena VBO+EBO e devolve uma mesh normal no registry. Normais passam por
  `normal_matrix` e são renormalizadas.
- `render3d/upscale.rs` — `Upscaler`: FBO com cor + profundidade `DEPTH_COMPONENT16`,
  upscale bilinear num quad full-screen. `RenderContext3D::set_render_scale(Option<f32>)`,
  `render_scale()`, `render_size()`; `drawable_size()` reporta o tamanho **baixo** para a
  câmara acertar o aspeto; `read_pixels` corre o upscale primeiro. `MIN_SCALE = 0.25`,
  `MAX_SCALE = 1.0`, e `1.0` é tratado como "sem upscale".
- `mini_fps`: cenário estático em dois batches (48 meshes → 2 draws, um por material),
  campo de 100 cubos instanciados (F2 alterna com o loop, `--cubes N` muda o tamanho),
  escala de render (F3 cicla 100/75/50 %, `--scale N`), `--bench` imprime uma linha
  legível por máquina e `--width/--height`.
- `benches/meshes_3d.rs` — 1/10/100/1000 cubos instanciados vs loop e 50 tiles estáticos
  com vs sem batch, uma frame completa por iteração.
- `tests/instancing.rs` (6), `tests/static_batch.rs` (3), `tests/upscale.rs` (5) — todos
  ao nível do framebuffer.
- ADR-044; `docs/PERF_LOG.md` com a secção "After Phase 17"; `docs/3D.md` §6.1
  "Upscaling".

### Resultados (detalhe e método em `docs/PERF_LOG.md`)

| | Antes | Depois | Ganho |
|---|---:|---:|---:|
| mini_fps draw calls (100 cubos + cenário) | 103 | **4** | 25.8x menos chamadas |
| mini_fps campo instanciado | 100 | **1** | — |
| x86_64 mini_fps, 100 cubos | 4391.7 FPS | **25776.0 FPS** | **5.87x** |
| criterion, 100 cubos | 109.81 µs | 9.01 µs | **12.19x** |
| criterion, 1000 cubos | 1023.5 µs | 57.07 µs | **17.94x** |
| criterion, 50 tiles estáticos | 65.08 µs | 9.73 µs | **6.69x** |
| PX30, 2000 cubos | 21.9 FPS (loop) | 59.0 FPS (instanciado) | capacidade |
| PX30, upscale 0.75 a 5000 cubos | 30.0 FPS | **48.4 FPS** | **+61.3 %** |

### O que falhou e ficou registado

O **upscaling não ajuda no x86_64**: 25776 → 18156 FPS a 75 % (−29.6 %). A frame a
25 000 FPS é limitada pela submissão, não pelo preenchimento, portanto um segundo
render target mais um blit é custo puro. Medido também a 2560x1440 (20995 → 14932) para
descartar a resolução como explicação. É o oposto do PX30, onde a mesma otimização dá
+61 % por a frame ser *fill-bound*. Está no `PERF_LOG` em vez de escondido.

### Gates da fase

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo clippy -p giochi-engine -p giochi-engine-3d -p giochi-demo --all-targets --features profiling -- -D warnings`
- [x] `scripts/gates.sh`
- [x] `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test instancing` — 6 testes
- [x] `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test static_batch` — 3 testes
- [x] `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test upscale` — 5 testes
- [x] `cargo bench -p giochi-engine-3d --no-run` e `-- --quick`
- [x] `CROSS_SYSROOT=/tmp/r35s-sysroot scripts/build-aarch64.sh --release -p giochi-engine-3d --example mini_fps`
      — corrido no PX30
- [x] `scripts/gen-header.sh --check` — `giochi.h` inalterado; a ABI v3 só chega na fase 18c

## Fase 16 — Batching 2D + Atlas
Data: 2026-11-08

**Escopo:** reduzir N `SDL_RenderCopy` por frame a poucos `SDL_RenderGeometry`. A API
pública não muda (R12): `draw_texture` mantém a assinatura e passa a agrupar.

### Entregue

- `giochi-engine/src/render/batch.rs` — `SpriteBatch`/`SpriteBatchConfig`. Agrupa
  quads em **runs consecutivos com a mesma textura** e emite uma chamada por run, por
  ordem de submissão. Não reordena: o renderer 2D do SDL não tem z-buffer, portanto
  ordenar por textura mudaria o resultado visual de sprites sobrepostos. Índices são
  **relativos ao run** (o SDL valida-os contra a fatia de vértices que recebe — índices
  absolutos faziam todos os runs depois do primeiro falhar). Um run de um único quad
  é desenhado com `RenderCopy`, não com geometria: não há nada para juntar e a cópia é
  mais barata. `DEFAULT_MAX_SPRITES = 1024`, escolhido por medição (ver abaixo).
- `giochi-engine/src/render/atlas.rs` — `TextureAtlas`/`AtlasRegion`: empacotador de
  prateleiras com 1 px de padding, ordenado por altura, dimensões potência de dois,
  limite `ATLAS_MAX_SIZE = 4096`. `from_images` (paths) e `from_rgba_images` (buffers
  em memória) partilham o empacotador, as regras de padding e a verificação de nomes
  duplicados.
- `RenderContext`: `draw_texture` enfileira; `flush_batch`, `draw_texture_immediate`,
  `sprite_batch`, `set_batch_config`, `load_atlas`. `flush_batch` é chamado por
  `present`, `read_pixels` (que passou a `&mut self`), `clear`, todas as primitivas não
  agrupadas, `destroy_texture`, `clear_resource_cache`, `set_batch_config` e pelo
  `Engine` no fim do scope `render2d` — nenhum método público pode ser observado com
  trabalho em fila. Corte por visibilidade aplicado antes de enfileirar.
- `FrameCounters::record_queued_sprite`/`record_draw_calls`, para a overlay contar
  sprites enfileirados e chamadas realmente emitidas.
- `giochi-engine/tests/render_batching.rs` — 1000 sprites → 1 chamada, pixels
  idênticos ao caminho imediato, ordem preservada, corte, flush automático no limite,
  atlas de ficheiro e atlas em memória → 1 chamada, sem fugas de textura.
- `giochi-engine/benches/sprites_2d.rs` — criterion, cenários `same_texture`,
  `same_texture_unbatched`, `ten_textures_grouped`, `ten_textures_interleaved`, `atlas`.
- `profiler_demo`: `--textures N`, `--use-atlas`, `--immediate`, `--centred-camera`,
  `--sprite-size`, `--batch-max`, e a linha de saída passou a incluir
  `renderer=`/`accelerated=`.
- ADR-043 em `docs/DECISIONS.md`; `docs/PERF_LOG.md` com a secção "After Phase 16".

### Resultados (detalhe e método em `docs/PERF_LOG.md`)

| | Draw calls | Antes | Depois | Ganho |
|---|---:|---:|---:|---:|
| x86_64 `opengl`, cena da fase 15, 1000 sprites | 1 | 2390.8 FPS | 6765.5 FPS | 2.83x |
| x86_64 `opengl`, `render2d` a 1000 sprites | 1 | 0.403 ms | 0.131 ms | **3.08x** |
| x86_64 `opengl`, campo inteiro, 1000 sprites | 1 | 896.9 FPS | 4024.8 FPS | 4.49x |
| criterion, 1000 sprites, 8x8 | 1 | 737 µs | 191 µs | 3.86x |
| PX30, 20000 sprites, 1 textura | 20 | 28.495 ms | 22.853 ms | −19.8 % |
| PX30, 5000 sprites, 10 texturas → atlas | 1 | 10.8 FPS | 58.6 FPS | **5.4x** |

Duas correções de cena que os números expuseram, e que ficam no demo:

1. O demo colocava os sprites em `0..width` com a câmara por omissão, que centra a
   origem do mundo no viewport — três quartos do campo ficavam fora do ecrã. O
   `--centred-camera` reproduz essa cena para comparar com a baseline da fase 15; o
   modo por omissão passa a fixar a câmara e a medir o campo inteiro.
2. A 24x24 a cena é *fill-bound*: 1000 sprites cobrem 576 000 pixels. Nenhum backend
   mostra ganho de FPS nessa configuração. `--sprite-size` e o cenário de 10 texturas
   contra atlas existem para medir onde as chamadas são de facto o gargalo.

### Regressão registada

No renderer `software` do SDL, com o campo inteiro, o agrupamento é **−3.1 %** a 1000
sprites e −8.3 % a 5000: nesse backend `RenderCopy` é um blit rápido escrito à mão e
`SDL_RenderGeometry` é um rasterizador de triângulos com interpolação por pixel. Está
dentro do limite de 10 %, a causa está nomeada no `PERF_LOG` e um host nesse backend
pode usar `set_batch_config(SpriteBatchConfig::new(_, 1))` ou `draw_texture_immediate`.
Na cena da fase 15 (a que a baseline registou) o mesmo backend **melhora 6.5 %**.

### Gates da fase

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo clippy -p giochi-engine -p giochi-engine-3d -p giochi-demo --all-targets --features profiling -- -D warnings`
- [x] `cargo test --workspace` — ver `scripts/gates.sh`
- [x] `cargo test -p giochi-engine --lib render::batch` — 17 testes
- [x] `cargo test -p giochi-engine --lib render::atlas` — 10 testes
- [x] `SDL_VIDEODRIVER=dummy cargo test -p giochi-engine --test render_batching`
- [x] `SDL_VIDEODRIVER=dummy cargo test -p giochi-demo --example profiler_demo --features profiling` — 9 testes do parser e da linha de saída
- [x] `cargo bench -p giochi-engine --bench sprites_2d --no-run` e `-- --quick`
- [x] `CROSS_SYSROOT=/tmp/r35s-sysroot scripts/build-aarch64.sh --release -p giochi-demo --example profiler_demo --features profiling` — corrido no PX30
- [x] `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test context_smoke`

## Sexta auditoria — os binários 2D tratavam um limite de frames mal escrito como "sem limite"
Data: 2026-11-08

Sexta passagem, sobre a superfície de linha de comando dos binários 2D publicados.

A segunda auditoria tinha corrigido isto nos **três exemplos 3D**: flags desconhecidas
eram ignoradas e um número inválido virava `None`, portanto `--frames=3oo` produzia uma
execução automática que nunca termina — num CI um timeout, num console um ecrã preto sem
mensagem. Os binários 2D tinham ficado com o parser antigo:

* `giochi-demo` — o binário que a release publica e que o aparelho executa;
* `mini_game` — o jogo de referência que todos os scripts de validação conduzem;
* `audio_test`, `input_test`, `moving_sprite`, `hello_rect`.

Os cinco passam a rejeitar flags desconhecidas, valor ausente e valor não numérico,
nomeando o flag e o valor. `giochi-demo --frames=3oo` agora imprime
`erro: --frames expects a number, got '3oo'` e sai com 1; antes arrancava um loop sem
limite. O parser do demo foi separado num `parse_from` testável com sete testes,
incluindo o caso `3oo` e a regra de que tudo depois de `lua` pertence ao script e nunca
é validado como flags do engine.

## Quinta auditoria — dois estados que um panic deixava presos, e uma leitura que podia paniquear
Data: 2026-11-08

Quinta passagem, desta vez sobre o ciclo de vida dos scenes na fronteira C e sobre o
carregador de assets. Cada correcção foi provada ao contrário, como nas passagens
anteriores.

### 1. Um panic dentro de `giochi_engine_run_scene` deixava o handle preso para sempre

O `guard` da fronteira converte um panic em `GIOCHI_ERROR_INTERNAL` — correcto. O que
estava errado era **tudo depois**:

* a marca `running` do scene era limpa por uma linha que só se alcançava no caminho sem
  panic. O unwind saltava por cima dela, e o handle passava a recusar
  `giochi_scene_destroy` («cannot destroy a scene while run_scene is running it») e
  qualquer segundo run («already running») — **para sempre**, porque nenhuma função
  reinicia o registo;
* `run_loop` esvaziava a pilha e entregava os `on_exit` adiados em linhas *depois* do
  run. O unwind saltava por cima delas também: o adaptador ficava na pilha com
  `on_stack == 1` e o `on_exit` nunca era entregue.

É a mesma classe de bug do `RUN_IN_PROGRESS` da segunda auditoria, corrigido com um
guard `Drop` pela mesma razão. Correcção: uma `RunningGuard` que limpa a marca em
qualquer caminho de saída, e um `catch_panic` interno ao `run_loop` que esvazia a pilha
e entrega os exits **antes** de gravar a mensagem de erro — a mensagem fica para depois
porque o `on_exit` do utilizador pode chamar funções `giochi_*`, e cada uma limpa o erro
à entrada.

Dois testes: um prova que o `Drop` limpa a marca; o outro constrói um engine, empilha um
adaptador, panica dentro do `run_loop` e afirma `INTERNAL`, pilha vazia, `on_exit`
entregue uma vez e o handle destrutível. Contra o código antigo o teste do `run_loop`
falha (o panic escapa).

Nota sobre o que **não** é alcançável: um panic *dentro de um callback C* não chega ao
`guard` — desde Rust 1.81 um panic num `extern "C" fn` é um abort do processo, por
definição da linguagem. A protecção cobre panics internos do engine, que é exactamente
o que o `guard` existe para conter.

### 2. Um asset que mudasse de tamanho entre o `stat` e o `read` fazia panic

`read_asset` fazia `fs::read` depois de `asset_size` e copiava com
`buffer[..bytes.len()]` sem verificar. Um ficheiro que **crescesse** entre as duas
syscalls — o buffer do chamador foi dimensionado pelo `stat` obsoleto — produzia
`range end index N out of range for slice of length M`, um panic em código seguro,
alcançável de C (`giochi_asset_read`) e do host Lua.

O próprio módulo reconhecia a corrida no `read_asset_vec` — mas com um `debug_assert`
que a release, a build que é publicada, compila fora.

Correcção: a cópia passou a um helper que verifica o tamanho lido contra o tamanho
estatado e devolve `GIOCHI_ERROR_ASSET` («changed size between stat and read») em vez de
panicar; o `read_asset_vec` ganhou o mesmo erro real. O panic antigo foi demonstrado com
as linhas exactas do código antigo antes de ficar.

## Quarta auditoria — seis defeitos de código e um teste frágil
Data: 2026-11-08

Quarta passagem. As três anteriores leram o ciclo de vida dos recursos, a validação da
fronteira C e a concordância contrato/header. Esta abriu o que nunca tinha sido aberto: as
conversões de valor do binding Lua, o snapshot de input 2D, o relógio do loop principal e o
caminho de afinação do jogo de balanço. Começou por um teste que estava **a falhar nesta
árvore**, e foi ele que levou ao primeiro item.

### 1. Todos os jogos começavam por simular um quarto de segundo que não existiu

`run_current` não reiniciava o `FrameClock`, cujo `last` é fixado em `Engine::new`. O
primeiro `tick()` de um run reportava tudo o que aconteceu entre a construção e o loop —
criação de janela, renderer e mixer, mais todos os assets que o chamador carregou antes de
arrancar — e isso era integrado como se fosse um frame. Medido: um run de 3 frames depois de
100 ms de preparação reportou **13** passos fixos e 0,131 s de jogo. Num console é a bola já
a rolar, o jogador já caído, e um primeiro `fps()` que é ficção.

O caminho de mudança de cena **já** reiniciava o relógio exactamente por esta razão; o
início do run é o mesmo acontecimento e tinha sido esquecido. Correcção de uma linha em
`run_current`, com teste que dorme 200 ms antes do loop e afirma **0 passos** no primeiro
frame — contra o código antigo dá 12.

**Como apareceu:** o `lua_smoke` falhava com «quit() ... got 2» tanto nesta árvore como em
`HEAD`. Investigar a causa revelou o relógio; a asserção do teste, porém, era ela própria
frágil (ver o item 6).

### 2. Um script Lua derrubava o processo com uma côr

`parse_hex_color` comparava `digits.len()` — **bytes** — e depois fatiava com intervalos de
bytes. `"日日"` tem 6 bytes e 2 caracteres, portanto passava o ramo `6 | 8` e o corte em
`0..2` partia um caractere ao meio: `byte index 2 is not a char boundary`, um panic Rust
levantado de dentro de um callback. O próprio `lib.rs` do binding diz que um script recebe
erros e não crashes. A string passou a ser validada como hexadecimal ASCII antes de ser
fatia­da, que é o que uma côr hexadecimal é por definição.

### 3. Um quit numa thread parava o engine de outra

`QUIT_REQUESTED` era um `AtomicBool` **global** enquanto o engine é thread-local (ADR-012):
duas threads com um engine cada partilhavam a flag, portanto `giochi_engine_request_quit`
na thread A terminava o loop da thread B, e dentro de um callback
`giochi_engine_is_running` respondia a partir do estado da outra thread. É o **mesmo**
defeito que o `RUN_IN_PROGRESS` tinha, encontrado da mesma maneira — e o comentário ao lado
do `RUN_IN_PROGRESS` apontava para ele desde a correcção anterior.

### 4. Teclas e botões ficavam presos ao perder o foco

O SDL não entrega `KeyUp` para uma tecla que estava premida quando o foco é tirado, e o
`apply_event` da segunda auditoria já tinha passado pelo `catch-all` sem notar que
`WindowEvent::FocusLost` caía lá dentro. Quem fazia alt-tab a meio de um passo continuava a
andar até o processo terminar, e um botão do rato premido continuava a ler-se como arrasto.
O `FocusLost` passou a limpar o estado retido dos dois snapshots e **deliberadamente** não
sintetiza arestas de release: um jogo ligado a «soltar para saltar» dispararia no alt-tab, e
o engine não distingue os dois casos.

### 5. `BalanceConfig::sanitised` guardava um campo em oito, e o próprio doc estava errado

Dizia que `fixed_step` «é o que importa»; um teste desmentiu-o.

| Campo | Valor «razoável» | O que acontecia |
|---|---|---|
| `fall_duration` | `0.0` («renascer já») | nunca satisfaz `fall_timer > 0.0` ⇒ nunca renasce; a bola fica fora do tabuleiro e `falls` sobe **por frame** (77 e a contar após 1 s) |
| `ball_radius` | `0.0` | ângulo de rolamento ÷ 0 ⇒ quaternion de spin `NaN` ⇒ bola invisível |
| `platform_size` | `0.0` / `-4.0` | `edge_proximity` = `0/0`; tamanho negativo ⇒ bola «caída» no centro, a ronda nunca começa |
| `damping` | `NaN` | `powf` propaga ⇒ velocidade e posição `NaN`, que já não compara como caída |

Todos os campos passam a ser trazidos para um intervalo executável, com aviso por
substituição, e `Ball::has_fallen`/`edge_proximity` deixaram de confiar num config que não
veio de `sanitised`.

### 6. A luz deixava passar um `ambient` `NaN`

`DirectionalLight::sanitised` usava `ambient.clamp(0.0, 1.0)`, e `f32::clamp` devolve `self`
para `NaN` — ao contrário de `f32::max`, que é por isso que `specular` e `shininess` já
estavam seguros. O `NaN` chegava a `u_ambient_strength`, o `gl_FragColor` ficava `NaN` em
todos os fragmentos e o frame ficava preto: exactamente a falha que a função documenta
prevenir. É alcançável a partir de C, onde `GiochiDirectionalLight` não tem construtor e um
campo esquecido é o que estava na pilha. Os três escalares passam a ser finitos, com tecto
finito no `specular` e no expoente.

### 7. Um teste afirmava uma propriedade temporal que o código não tem

O `lua_smoke` exigia que `quit()` parasse o run em **pelo menos 3 frames de display**, mas
`update` corre por passo fixo e um frame absorve todos os passos que o seu tempo cobre —
incluindo o trabalho do primeiro frame. Nesta máquina o primeiro `present` custou ~120 ms e
o run fechou em 2 frames. Passou a afirmar o que o contrato diz: a condição de quit disparou
(`update` correu ≥ 3 vezes) e o run parou muito antes do limite de 500 frames.

### Método

Cada correcção foi provada ao contrário: o teste novo foi executado **contra o código
antigo** e confirmado a falhar, antes de ficar. Os seis casos têm teste de regressão — um
deles no consumidor C (`c_smoke_3d.c` passa uma luz com `NAN`/`INFINITY` em todos os
campos), porque é aí que o defeito é alcançável.

## Terceira auditoria — a luz estava no contrato e não no header
Data: 2026-11-08

Terceira passagem, agora a comparar **três** descrições da mesma fronteira entre si: o
contrato (`docs/C_ABI_CONTRACT.md`), o header gerado (`giochi.h`) e os símbolos exportados
pelo `libgiochi.so`. Uma delas tinha de estar errada, e duas estavam.

### 1. `GiochiDirectionalLight` não existia no header

A §9.4a do contrato define a struct desde a fase 13, com tamanho e alinhamento escritos, e
o `giochi.h` afirma ser essa mesma especificação. Mas o `cbindgen.toml` listava apenas
`giochi-engine` em `include` — a crate `giochi-engine-3d` não estava lá, portanto o tipo
nunca era emitido. Um consumidor C que lesse o contrato encontrava uma struct que o
compilador dizia não existir.

### 2. `giochi_renderer3d_set_light` estava documentada e não implementada

A §9.4a descrevia o comportamento da função e a §10.6 remetia para ela; a função não estava
em lado nenhum do crate. O efeito prático: `GIOCHI_SHADER_PHONG` era anunciado como um dos
três shaders que o engine traz, e o seu único input era inalcançável a partir de C. O
consumidor ficava com o sol por omissão e sem forma de o mover.

Isto é o inverso do `draw_mesh`, que tinha o problema oposto na fase 14 (a assinatura não
tinha o parâmetro que a função precisava, e o contrato foi corrigido). Aqui o contrato já
estava certo; faltava o código que lhe correspondia.

Correcção: a função passa a existir, com `validate_engine` igual à das outras (NULL →
`INVALID_ARGUMENT`, ponteiro que não é o engine vivo → `INVALID_HANDLE`) e a mesma
sanitização do `RenderContext3D::set_light`, para que um chamador não consiga produzir um
frame preto por deixar `shininess` a zero (`pow(x, 0)` é 1 em todo o lado) ou uma direcção
de comprimento nulo (que normaliza para NaN).

### Como foi verificado

- `c_smoke_3d.c` passou a **afirmar os tamanhos do contrato em tempo de compilação**:
  `_Static_assert(sizeof(GiochiDirectionalLight) == 88)`, e 28/32 para `GiochiVec3` e
  `GiochiColor3D`. Assim é o compilador C — o lado que realmente faz o layout — que detecta
  um desvio, e não um teste Rust que concorda com o layout Rust.
- O mesmo teste exercita a função: luz válida aceite, `NULL` recusado com mensagem,
  engine destruído devolve `INVALID_HANDLE` em vez de *use-after-free*, e uma luz toda a
  zeros não impede o frame seguinte de desenhar.
- `python_smoke_3d.py` passa uma luz pela fronteira a partir de uma **segunda**
  implementação de FFI, que é onde um erro de layout por ponteiro apareceria.
- Contra o header antigo o teste **não compila** — é essa a prova de regressão, verificada
  antes de a correcção ficar.

### O que ficou por verificar, e porquê

O `libgiochi.so` ARM64 compilado exporta **74** símbolos `giochi_*`, entre eles
`giochi_renderer3d_set_light`, e os dois consumidores C (`c_smoke.c`, `c_smoke_3d.c`)
compilam e ligam para aarch64 contra ele. **A execução no console não aconteceu:** a meio
do deploy o aparelho desapareceu da rede — o adaptador USB-Ethernet `enx0247494f4348`
deixou de existir no host, portanto foi o cabo ou a alimentação, não o SSH. O script foi
interrompido.

Consequência a ter em conta quando o aparelho voltar: o `hardware-check.sh` liberta o
console fazendo `chmod 0644` no launcher, e essa alteração **persiste** se a energia cair
antes do `restore_console`. Se o ecrã ficar preto no próximo arranque:

```sh
ssh root@10.55.0.2 'chmod 0755 /opt/giochi/apps/launcher/bin/giochi-launcher;
                    rm -f /run/giochi/launcher-parado;
                    for p in $(ps | grep "[g]iochi-init" | awk "{print \$1}"); do
                        kill -TERM "$p"; done'
```

O resto da validação — os quatro consumidores em x86_64 (debug e release), os gates
completos, o `check-aarch64.sh` e os dois jobs de CI — passou.

### O que a terceira passagem confirmou estar correcto

Os tamanhos `28/32/88` batem entre Rust, contrato e C; o alinhamento é 4 em todos; o
`_reserved` continua obrigatório e ignorado; e as quatro funções que tomaram
`validate_engine` na segunda passagem usam-no pela mesma ordem (engine primeiro, argumento
depois), que é a ordem que o `draw_mesh` já tinha.

## Uma falha de CI que era bug real, não flake
Data: 2026-11-08

`unknown_handles_are_reported_not_crashing` começou a falhar no CI e a passar localmente —
a assinatura de um flake, e não era.

`RUN_IN_PROGRESS`, a flag que recusa um `giochi_engine_run` reentrante, era um `AtomicBool`
**global** enquanto o engine é thread-local (ADR-012). Duas consequências:

1. Um run na thread A fazia o `giochi_engine_run_scene` da thread B falhar com "already
   running **on this thread**" — falso, e sem nada que o chamador pudesse corrigir.
2. Os próprios testes do capi corriam em corrida: um teste a entrar num run virava a flag
   enquanto um teste paralelo verificava um código de erro não relacionado. O CI apanhou
   `-7` (SCENE) onde esperava `-1` (INVALID_HANDLE).

A flag passou a thread-local, e o par set/reset passou a ser um guarda com `Drop`: um panic
dentro do loop desenrolava por cima do reset e deixava a thread permanentemente "a correr",
depois do qual **todos** os runs e steps seguintes eram recusados como reentrantes, sem
recuperação possível.

Dois testes de regressão cobrem os dois casos — um run noutra thread, e um panic dentro de
um — e **ambos foram verificados a falhar contra o código antigo** antes de a correcção
ficar.

## Segunda auditoria — 4 defeitos adicionais
Data: 2026-11-08

Segunda passagem, focada nas áreas que a primeira não cobriu: fronteira C, ciclo de vida de
recursos 2D, binding Lua e parsing de argumentos.

### 1. 2D e 3D não podiam coexistir, e diziam-no na língua errada

Ambos os caminhos de criação pedem inicialização de vídeo ao SDL, e o SDL permite **um
`EventPump` por processo**. O segundo a arrancar falhava com
`an `EventPump` instance is already alive` — um tipo interno que o chamador nunca usou, e
sem dizer a regra que se aplica (ADR-037: escolhe-se um renderer, não se misturam). Agora
cada lado recusa com uma mensagem que nomeia a regra e a chamada que resolve, verificado a
partir de C nas **duas** direcções.

**Como apareceu:** foi o meu próprio teste unitário de GL a falhar ao criar um segundo
`Platform`, e o texto do erro era sobre um `EventPump`. Sem isso, um consumidor C veria
isto e não teria por onde começar.

### 2. A C ABI 3D desreferenciava qualquer ponteiro não nulo

`giochi_engine3d_begin_frame`, `_end_frame`, `giochi_renderer3d_clear` e `_draw_mesh`
testavam apenas NULL — sob um comentário `SAFETY` que afirmava que o ponteiro era
"asserted to be the thread's current engine by the same rule the other functions use".
**Essa verificação não existia.** Como `create` recusa um segundo engine por thread, há
exactamente um ponteiro válido em cada momento, portanto a comparação é possível e barata:
um ponteiro obsoleto ou de outra thread passa a devolver `GIOCHI_ERROR_INVALID_HANDLE` em
vez de ser desreferenciado.

### 3. `release_all` declarava sucesso quando algo tinha ficado por libertar

Respondia "os registos estão vazios?", que é pergunta diferente de "os objectos GL foram
apagados?": quem tivesse um clone do `Rc` fazia o `destroy_*` registar um leak enquanto a
entrada desaparecia de qualquer maneira. Passou a contar as referências primeiro e devolve
`false` quando alguma vai ficar por libertar — o comportamento que a **sua própria
documentação já prometia**.

### 4. Um erro de escrita num argumento significava "sem limite de frames"

Flags desconhecidas eram ignoradas e um número inválido virava `None`, portanto
`--frames=3oo` produzia uma execução automática que nunca termina: num CI isso é um
timeout, não uma mensagem. `--frames`, `--trace`, `--obj` e `--quadro` passam a rejeitar
valor ausente ou malformado, flags desconhecidas são erro, e a mensagem lista os flags
realmente aceites em vez de sugerir um `--help` que não existe.

### O que a segunda passagem confirmou estar correcto

`Quat::normalized` devolve a identidade para um quat nulo; `Transform3D::matrix` normaliza
antes de compor; o `Drop` do `AudioSystem` chama `clear`, que pára música e sons antes de
libertar; o parsing de argumentos não indexa vectores; e os scripts não repetem o bug de
parsing que a bateria tinha.

## Auditoria profunda do repositório — 4 defeitos reais
Data: 2026-11-08

Auditoria pedida pelo utilizador. Método: lints agressivos (`clippy::pedantic` e
`nursery`), varrimento de `unsafe`/`unwrap`/`panic`/`TODO` fora de módulos de teste, e
leitura dos caminhos críticos (registos de handles, mixer, loop de jogo, física, ABI).

O que **não** havia, e vale registar: zero `unsafe` fora das chamadas GL obrigatórias,
zero `unwrap`/`expect`/`panic!` em código de produção em todas as crates, zero
`TODO`/`FIXME`, e uma única supressão de lint em todo o código (um `module_inception`
cosmético).

### 1. Todos os meshes e shaders 3D perdiam os objectos GL (o mais grave)

`destroy_mesh` e `destroy_shader` liam o recurso com `HandleRegistry::get`, que
**clona** o `Rc` — contagem 2 — e depois pediam `Rc::try_unwrap`, que exige contagem 1.
Nunca podia ter sucesso. A entrada do registo desaparecia, mas o VAO, VBO, EBO e o
programa ficavam vivos no driver. O único rasto era um `log::warn!` que nenhum exemplo
mostra, porque nenhum inicializa logger.

Pior: os exemplos imprimiam `released: 0 meshes and 0 shaders left`, que conta **entradas
do registo**, não objectos GL. Lia-se como confirmação de uma limpeza que não acontecia.
O `release_all` tinha o mesmo bug, por ser construído sobre as mesmas duas chamadas.

Correcção: `HandleRegistry::take` remove e devolve a única referência. O `remove` foi
**eliminado** — é a forma que convida ao erro. Teste:
`destroying_a_resource_really_deletes_its_gl_objects` pergunta ao driver com
`glIsBuffer`/`glIsProgram` antes e depois, e foi verificado a falhar contra o código antigo.

### 2. `destroy_sound` libertava um `Mix_Chunk` que podia estar a tocar

O SDL_mixer guarda o ponteiro do chunk enquanto um canal o reproduz; libertá-lo a meio é
use-after-free na thread do mixer — áudio corrompido e depois crash. O `destroy_music`
sempre fez `halt()` antes; o caminho dos sons foi esquecido. Corrigido, e o doc explica
porque tem de ser todos os canais: a crate `sdl2` não permite perguntar *qual* chunk um
canal tem (`Chunk::PartialEq` inclui a flag de posse e o ponteiro cru é privado).

### 3. `fixed_step` a zero (ou negativo) pendurava o jogo para sempre

Os campos de `BalanceConfig` são públicos e `BalanceGame::update` faz
`while accumulator >= fixed_step` subtraindo o passo: zero nunca progride, negativo sobe o
acumulador. O jogo congelava sem erro e sem frame. `BalanceConfig::sanitised` limita a
1 ms e `BalanceGame::new` aplica-o. O teste afirma o valor **antes** do `update`, para uma
regressão falhar em milissegundos em vez de consumir o timeout do CI.

### 4. `draw_line` ignorava o guarda de coordenadas

Convertia com `as i32` enquanto todos os outros caminhos do módulo passam por
`to_sdl_coord`, que mapeia não-finitos para 0 e limita o resto. O `as` do Rust satura,
portanto um extremo infinito virava `i32::MAX`/`i32::MIN` e era entregue ao clipping
inteiro do SDL. (Para `NaN` os dois caminhos dão 0, e o comentário da correcção di-lo em
vez de exagerar o caso.)

### 5. Documentação que não correspondia ao código

`destroy_music` afirmava parar a reprodução apenas quando o handle era a faixa a tocar.
Para sempre, porque não há API para saber qual toca. O código estava certo e a frase não.

### Um falso-verde apanhado durante a própria auditoria

A primeira versão dos testes de GL criava dois `Platform` no mesmo processo; o segundo
falha, o teste imprimia "skipped" e o resultado aparecia como **`ok`**. Um teste saltado
reportado como verde é precisamente a classe de problema que esta auditoria procurava, e
foi corrigido fundindo tudo num único teste com um só contexto.

## Segunda validação em hardware **diferente**: Powkiddy x55 (Mali-G52)
Data: 2026-11-07

O aparelho ligado em `10.55.0.2` passou a ser um **Powkiddy x55** — não o PX30/R35S.
Confirmado por `hostname` (`giochi-system-x55`) e pelo kernel: `panfrost ... mali-g52 id
0x7402`, num RK3566. **Toda a bateria de testes passou**, sem alterar uma linha de código:

| Check | PX30 / Mali-G31 | **x55 / Mali-G52** |
|---|---|---|
| `demo --verify` | OK | **OK** |
| `demo --quadro` | 640×480 | **720×1280** (painel retrato) |
| `lua mini_game` | 59,8 fps, 0 texturas vivas | **59,9 fps, 0 texturas vivas** |
| `c_smoke` (parte I) | `abi 2, engine 0.2.0` | **`abi 2, engine 0.2.0`** |
| **`c_smoke_3d` (parte II)** | **ok** | **ok** |
| `cube_demo` | GLES 2, VAO sim, 0 erros | **GLES 2, VAO sim, 0 erros** |
| `mini_fps` | Phong, 24 caixotes | **Phong, 24 caixotes** |
| `ball_balance` | p50 1,07 / p99 3,77 ms | **p50 1,10 / p99 3,95 ms** |

Isto vale mais do que repetir o teste no mesmo aparelho: **dois SoCs, duas GPUs e duas
geometrias de painel** diferentes (640×480 paisagem, 720×1280 retrato), com o mesmo binário
compilado uma vez. É a evidência de que a camada 3D e o ABI não estão afinados para um
aparelho. Em particular, o `aspect` derivado do drawable (ADR-035) e o `BalanceView`
testado a 4:3 e 16:9 sobrevivem a um painel de 9:16 sem ajuste.

### Bug corrigido no `hardware-check.sh` (encontrado por esta troca)

O registo da bateria era escrito separado por espaços e lido com `read -r a b c d`. Neste
aparelho o `status` é **"Not charging"** — com espaço — portanto todos os campos depois
dele deslizaram: a tensão passou a ser a string `"charging"` e o mínimo ficou com o valor
da tensão. Passou despercebido porque a condição de recusa não disparou por acidente
(`charging=yes`). O registo passou a usar `|` como separador e o código passou a avisar
quando a capacidade não é numérica, em vez de falhar sob `set -e`.

## Validação da C ABI v2 no PX30 (Mali-G31) — completa
Data: 2026-11-07

A parte II do ABI estava validada em x86_64 (Mesa/EGL) mas não no console. Com o
aparelho de volta (18 % de bateria, 4,048 V — desta vez acima do mínimo de projecto),
correu o `scripts/hardware-check.sh` completo, que agora compila, instala e executa o
consumidor C da parte II.

Todos os checks passaram:

| Check | Resultado no Mali-G31 |
|---|---|
| `demo --verify` | `bootstrap OK` |
| `demo --frames 120 --quadro` | 640×480, 60,0 fps |
| `lua mini_game` | 59,8 fps, pontuação 1, 3 vidas, **0 texturas vivas** |
| **`c_smoke` (parte I)** | **`OK (abi 2, engine 0.2.0)`** |
| **`c_smoke_3d` (parte II)** | **`ok`** |
| `cube_demo` | GLES 2, VAO sim, 0 erros GL |
| `mini_fps` | Phong, **24 caixotes baixos**, 0 erros GL |
| `ball_balance` | p50 1,11 ms, p99 4,74 ms, 0/120 acima de 20 ms |

O `c_smoke_3d` é a prova que faltava: um programa **escrito em C**, compilado com
`-Wall -Wextra -Werror` contra o `giochi.h` gerado, ligado ao `libgiochi.so` do
aarch64, a criar um engine 3D, desenhar três frames de um cubo iluminado, exercitar
todos os caminhos de erro e desmontar tudo — no hardware. A parte II do ABI funciona
numa linguagem que não é Rust, no dispositivo.

## Release `v0.2.0` — re-apontada para levar a documentação da 2.ª validação
Data: 2026-11-07

A tag foi primeiro criada em `f8f1aeb`, antes de existir o registo da validação no
Powkiddy x55. Isso deixava o arquivo publicado com um `PROGRESS.md` que **não mencionava**
essa validação — enquanto o corpo da release dizia, textualmente, "See ... PROGRESS.md for
what was validated where". A release apontava para um documento que, dentro do próprio
arquivo, não dizia o que ela afirmava.

Corrigido re-apontando a tag para `cb1eb19`. Entre os dois commits só mudam `PROGRESS.md`
e `scripts/hardware-check.sh`, e o segundo nem sequer viaja no arquivo — portanto **os
binários são equivalentes**; o que muda é a documentação embarcada. Verificado depois do
rebuild: o `PROGRESS.md` do arquivo publicado menciona o x55 e o Mali-G52, o demo arranca,
e os dois consumidores C compilados contra o `include/` do arquivo ligam ao `lib/` do
arquivo e passam (`OK (abi 2, engine 0.2.0)` e `ok`).

## Release `v0.2.0` — publicada e verificada a partir do artefacto publicado
Data: 2026-11-07

Publicada (não draft) com os dois arquivos. A verificação foi feita **sobre o que está
publicado**, descarregado do GitHub — não sobre a build local:

| Verificação no arquivo x86_64 publicado | Resultado |
|---|---|
| `giochi-demo --verify` | `bootstrap OK` |
| `giochi-demo lua lua/mini_game.lua` | pontuação, 3 vidas, **0 texturas vivas** |
| `c_smoke` compilado contra o `include/` do arquivo e ligado ao `lib/` do arquivo | **`OK (abi 2, engine 0.2.0)`** |
| `c_smoke_3d` (parte II) contra o mesmo | **`ok`** |

No arquivo ARM64 publicado: todos os binários — `giochi-demo`, `lib/libgiochi.so`,
`bin/cube_demo`, `bin/mini_fps`, `bin/ball_balance` — são ELF **ARM aarch64**; o
`giochi.h` diz `GIOCHI_ABI_VERSION 2` e `0.2.0`; e o `libgiochi.so` exporta as **16**
funções da parte II.

**O arquivo ARM64 não pôde ser executado no console.** A bateria colapsou de 18 % para
**0 %, 0,019 V** durante a sessão. O carregador passou a `online=1` e a carregar a
**2,2 mA** — um trickle de pré-carga, que num pack de 4 Ah levaria semanas. Correr GLES
nessas condições é o cenário exacto que já apagou o rootfs deste aparelho duas vezes,
portanto não foi feito. O que sustenta a release ARM64 é a validação feita **20 minutos
antes**, com a mesma fonte (ver a secção acima) mais a verificação estrutural do arquivo.

### Três bugs do workflow de release, encontrados porque a verificação finalmente correu

O passo `verify-release-artifact.sh` tinha sido escrito depois de a `v0.1.1` já estar
publicada, e nunca tinha executado. Quando executou:

1. **O job `release` não fazia checkout** — não tinha o script.
2. **`cargo build --workspace --examples` filtra os targets** em vez de os acrescentar:
   compilava os exemplos e **saltava** o `giochi-demo` e o cdylib `libgiochi`, deixando
   uma biblioteca de 0.1.1 sob um header de 0.2.0. Apanhado pela comparação do header do
   arquivo com a biblioteca do arquivo.
3. **O job de publicação não tinha SDL2** — `undefined reference to SDL_*`.

E dois na própria publicação:

4. **`generate_release_notes: true` com `body_path` duplicava a página** a cada re-execução.
5. **`action-gh-release` não reescreve o body de uma release existente.** A tentativa de
   correcção anterior removeu a causa mas não o que já estava escrito, e **reportou
   sucesso** com a página ainda errada. Resolvido com um passo `gh release edit
   --notes-file` depois, que torna a re-execução idempotente.

## Release `v0.2.0` — verificada e publicada
Data: 2026-11-07

A tag `v0.2.0` foi criada, o workflow `Release` correu verde e a release está
publicada **como draft**, à espera de revisão humana — que é o que o próprio
workflow faz de propósito.

Publicar não foi imediato: a verificação do arquivo apanhou **dois bugs reais** no
workflow, que nunca tinham executado porque o passo de verificação foi escrito depois
de a `v0.1.1` já estar publicada.

1. **O job `release` não fazia checkout.** Ficava sem `scripts/verify-release-artifact.sh` e
   falhava com "No such file or directory".
2. **`cargo build --workspace --examples` filtra os targets em vez de os acrescentar.**
   Compilava os exemplos e as dependências deles, e **saltava** o `giochi-demo` e o
   cdylib `libgiochi`. Os dois ficaram artefactos de 0.1.1 enquanto o `giochi.h` dizia
   0.2.0. Foi a verificação do arquivo — que compara o header do arquivo com a biblioteca
   do arquivo — que o apanhou, com `giochi_abi_version() == GIOCHI_ABI_VERSION` a dar
   1 contra 2.
3. **O job de publicação não tinha SDL2.** O `libgiochi.so` tem `NEEDED` para SDL2,
   SDL2_image e SDL2_mixer, portanto compilar e correr o consumidor C sem elas dá uma
   parede de "undefined reference to SDL_*".

Depois das três correcções, a verificação local do arquivo passa de ponta a ponta:
header 0.2.0, **73 símbolos `giochi_*` exportados**, `c_smoke (OK (abi 2, engine
0.2.0))`, e o demo e o jogo Lua continuam a funcionar com os PNG apagados.

Os três exemplos 3D passam a viajar em `bin/` nos dois arquivos, e o job ARM64 verifica
que são realmente AArch64 — 3D como biblioteca partilhada que ninguém consegue correr é
meia entrega.

## Entregue na fase 14 — C ABI 3D, parte II (`GIOCHI_ABI_VERSION 2`)
Data: 2026-11-07

Pedido: expor o 3D via C ABI, regenerar `giochi.h` com a versão 2, e prová-lo com
consumidores que não são Rust.

### O que existe

- `giochi-engine-capi/src/lib3d.rs` — 17 funções e o tipo opaco `GiochiEngine3D`.
- `giochi.h` (1331 linhas) com `#define GIOCHI_ABI_VERSION 2`, a declaração opaca
  `typedef struct GiochiEngine3D GiochiEngine3D;`, e os tipos de valor 3D
  (`GiochiVec3` 28 B, `GiochiQuat` 32 B, `GiochiColor3D` 32 B, `GiochiTransform3D`
  104 B, `GiochiCamera3D` 116 B, `GiochiWindowConfig` 44 B).
- `GIOCHI_SHADER_FLAT` / `_TEXTURED` / `_PHONG`.
- Quatro consumidores reais, todos verdes: `c_smoke.c` e `python_smoke.py` (parte I,
  provando que não regrediram), `c_smoke_3d.c` e `python_smoke_3d.py` (parte II).
- `tests/rust_ffi_3d.rs` — 8 testes, incluindo o layout dos tipos e o ciclo completo.
- `docs/C_ABI.md` §9 (tutorial 3D completo), ADR-041, contrato §10 revisto.

### Três decisões tomadas ao implementar

1. **O engine implícito.** `giochi_mesh_*` e `giochi_shader_*` não levam engine no
   contrato, mas um mesh é um objecto GL e destruí-lo exige o contexto que o criou. A
   regra passou a ser "o engine criado mais recentemente nesta thread", e **criar um
   segundo engine na mesma thread é recusado**. Sem essa recusa, "mais recente" seria
   uma regra que o chamador podia quebrar por acidente: criar A, criar B, destruir um
   mesh de A, e o destroy olhava para o registry de B e dizia `INVALID_HANDLE` para um
   handle perfeitamente válido. Com um engine por thread o mapeamento é bijecção.
   (ADR-041)
2. **`draw_mesh` ganhou um `GiochiColor3D`.** O contrato não o tinha. Um mesh na parte II
   é geometria sem material, portanto sem cor todos os draws usavam a cor de fundo e
   ficavam invisíveis. A parte II ainda não tinha saído — é exactamente a altura em que
   um contrato se corrige em vez de se contornar com estado escondido. (contrato §10.7)
3. **A versão passou a ser `>=`, não `==`.** O `c_smoke.c` da parte I afirmava
   `giochi_abi_version() == 1` e ficou vermelho assim que a versão subiu para 2 — o que é
   o comportamento correcto de um teste a apanhar uma regra errada. Um consumidor da
   parte I tem de testar "pelo menos a versão com que fui compilado"; é isso que está
   agora escrito, no teste e no `giochi.h`.

### Adições ao contrato além da tabela §10

- `giochi_transform3d_identity` e `giochi_transform3d_from_translation`: um
  `GiochiTransform3D` zerado tem **escala zero** e não desenha nada — uma armadilha que em
  Rust não existe (`Default` é o identidade) mas que em C se escreve sem dar por isso.
- `giochi_mesh_create_cube` e `giochi_shader_builtin`: sem elas, o menor programa C 3D
  precisava de um OBJ e de três ficheiros GLSL em disco. O §11 já dizia "ficheiros ou
  built-ins", portanto a segunda metade estava por cumprir.

### Nota de verificação

Validado em x86_64 (Mesa/EGL, `offscreen`) com os quatro consumidores. **Não validado no
PX30** porque o aparelho estava inacessível durante a fase; o
`scripts/hardware-check.sh` passou a compilar, instalar e correr `c_smoke_3d` no console,
portanto a validação acontece no próximo deploy.

## Validação de `ball_balance` no PX30 (Mali-G31) — dois bugs encontrados
Data: 2026-11-07

O jogo foi finalmente executado no aparelho. Correr no hardware encontrou dois bugs que
o desktop não podia mostrar, e ambos invalidavam exactamente aquilo para que o jogo
existe.

### 1. O relógio do trabalho por frame era lido depois do swap

`end_frame` no KMSDRM bloqueia até ao vsync. Medir depois dele cobra a **todos** os
frames os 16,7 ms de espera, portanto:

- o indicador de engasgo ficava permanentemente no limiar (16,7 ms), nem verde nem
  vermelho, sempre a dizer que o jogo ia a falhar;
- os percentis reportados (`frame work: p50 16.67`) descreviam o painel a refrescar e
  não o jogo.

Medido depois da correcção, no Mali-G31: **p50 1,07 ms, p99 3,77 ms** em 240 frames,
0 frames acima de 20 ms. Ou seja, o jogo tem ~13 ms de folga a 60 Hz. O `scripts/
hardware-check.sh` agora falha se o p99 reportado se aproximar de um intervalo de
refrescamento, para isto não voltar.

O `--trace` passou a chamar ao campo `work` em vez de `dt`: é o custo do jogo, não um
intervalo entre frames.

### 2. Os dois indicadores de latência estavam fora do campo de visão

O painel do console é 640×480 — **4:3**, portanto mais estreito do que uma janela
1280×720. Os marcadores tinham sido colocados a olho num ecrã 16:9:

| Marcador | ndc a 16:9 | ndc a 4:3 |
|---|---|---|
| Indicador de engasgo | −0,90 | **−1,21 → fora do ecrã** |
| Marcador de input, centrado | +0,64 | +0,85 |
| Marcador de input, stick a fundo | +0,90 | **+1,21 → fora do ecrã** |

Confirmado na captura: a 640×480, **zero** pixéis azuis e **zero** verdes. O
utilizador veria a plataforma e a bola e nenhum dos dois indicadores que explicam de
onde vem o atraso.

Correcção: a câmara e as âncoras dos marcadores passaram para
`giochi_engine_3d::game::BalanceView`, e `every_marker_stays_on_screen` projecta os dois
marcadores através dessa câmara a **1280×720 e 640×480**, em toda a gama do stick,
exigindo `|ndc| < 0,85`. Depois da correcção, na captura do aparelho: marcador azul
1482 px, indicador verde 1482 px, ambos dentro do quadro.

### Instalado como aplicação do console

O jogo não ficou só num directório: está registado no lançador, portanto aparece no menu
do aparelho e arranca com o comando, sem SSH.

| O quê | Onde |
|---|---|
| Catálogo | `/opt/giochi/apps/index.json` (entrada `equilibrio`; cópia de segurança em `index.json.bak`) |
| Instalação | `/user/apps/equilibrio/` — `bin/ball_balance`, `metadata.json`, `capa.png` |
| Cópia de trabalho | `/opt/giochi/bin/ball_balance` (a que os scripts de teste usam) |

Verificado no aparelho: depois de reiniciar o lançador, ele arrancou o jogo sozinho —
`/proc/<pid>/cmdline` = `/user/apps/equilibrio/bin/ball_balance`, `cwd` =
`/user/apps/equilibrio`, RSS 20 MB, 11 % de um núcleo (bloqueado no vsync a 60 Hz). Ao
terminar, o lançador recuperou o DRM master e voltou ao menu. O ciclo completo funciona.

A `capa.png` (320×320 RGBA, o mesmo formato da do Cobrinha) é um placeholder gerado por
código — a plataforma inclinada e a bola, nas cores do jogo. Vale a pena substituí-la.

### 3. `Mat4::transform_point` não divide por `w`

Encontrado ao escrever o teste acima. Espelha o `transform_point3` do `glam`, que
**afirma** que a linha de baixo é `[0, 0, 0, 1]` — ou seja, é só para matrizes afins.
Passar-lhe uma matriz de projecção devolve coordenadas de *clip*, que se parecem com
números normais. Foi por isso que os marcadores fora do ecrã passaram despercebidos.
`Mat4::project_point` faz a divisão e devolve `None` para um ponto atrás do olho; o
`transform_point` ficou documentado como afim.

## Entregue na fase 13 — Iluminação direcional (Phong)
Data: 2026-11-07

Pedido (opcional no prompt da extensão v2.1, mas implementado): luz direcional com
ambiente, difusa e especular, para a face de cima de um cubo se distinguir de uma
lateral sem recorrer ao wireframe.

### O que existe

- `render3d/light.rs` — `DirectionalLight`, `#[repr(C)]`, 88 bytes, só `f32`/`u32`.
  `sun()` dá um sol branco inclinado; `with_*` para o resto; `sanitised()` é o que o
  renderer guarda.
- `shaders/phong.vert` / `phong.frag` — GLSL ES 1.00, Blinn-Phong. As varyings são
  **world space**: em view space o sol passaria a seguir a câmara.
- `Mat4::normal_matrix()` — `transpose(inverse(mat3(model)))` na CPU, porque GLSL ES
  1.00 não tem `inverse()`.
- `BuiltinShader::Phong` + `reads_lighting()`; `RenderContext3D::set_light` / `light()`.
- `tests/lighting.rs` — 7 testes que leem o framebuffer de volta.
- `mini_fps` passa a desenhar com Phong e um sol explícito, e imprime os parâmetros.
- `docs/3D.md` §5.1 "Lighting", ADR-040, secção 9.4a do `docs/C_ABI_CONTRACT.md`.

### Três decisões que não são óbvias (ADR-040)

1. **Uma luz por frame, no renderer.** Por mesh exigiria registo, handle e tempo de vida
   para algo que todas as cenas deste repositório definem uma vez. O custo por draw é de
   cinco uniforms de qualquer maneira.
2. **`direction` é o sentido em que a luz viaja.** `(0, -1, 0)` ilumina um chão visto de
   cima. O shader nega o vector uma vez. A convenção contrária é igualmente comum, e é
   por isso que está escrita em dois sítios.
3. **Matriz de normais na CPU.** O shader `flat` da fase 11 usava `mat3(u_model)` e estava
   correcto só por acidente: nada até à fase 12 escalava um objecto de forma não uniforme.

### Bug encontrado ao construir isto

**Os primeiros testes passavam pela razão errada.** As bandas de amostragem
(`0.58..0.86` e `0.14..0.42` da altura) ficavam **fora** da silhueta da esfera — o
projectado tem ~52 px de raio numa janela de 120 px, e a câmara a 5 unidades punha o
equador no centro. O teste comparava "superfície iluminada" com "fundo preto" e passava
com a luz apontada para qualquer lado. A correcção foi aproximar a câmara para 2 unidades
(silhueta de ~60 px, o equador continua em `y = HEIGHT/2`) e amostrar um **rectângulo**
estreito sobre o eixo vertical, que se prova dentro da silhueta. O perigo real é que o
teste era verde: um teste de pixel satisfeito pela geometria errada é pior do que nenhum.

### O critério de aceitação não era observável

O prompt pede "`mini_fps` com cubos iluminados (face superior mais clara que lateral)".
Durante a fase 13 isso não era observável, e nada o dizia: o `mini_fps` desenhava
obstáculos de 2 m vistos por um caminhante com os olhos a 1,7 m, portanto a face de cima
de cada cubo ficava sempre **acima** da câmara e só se viam as laterais. Os testes de
iluminação verificavam o comportamento num cubo colocado por eles próprios — é um bom
teste unitário e não é a verificação de aceitação.

Correcção:

- O `mini_fps` passa a desenhar **24 caixotes baixos** (0,6 m, entre os obstáculos), e
  reporta quantos desenhou. É isso que põe uma face de cima no ecrã enquanto se caminha, e
  o CI passa a exigir que sejam mais de zero.
- `a_crates_top_face_reads_brighter_than_its_side_the_phase_13_acceptance_check` mede-o com
  os números do próprio exemplo — tamanho do caixote, altura dos olhos e a luz exacta do
  `mini_fps` — projectando um ponto na face de cima e outro na frontal com
  `Mat4::project_point` e amostrando um pequeno quadrado à volta de cada. Projectar em vez
  de adivinhar onde as faces calharam é o que impede as amostras de caírem no fundo.

### O que a fase 13 não muda

`flat` e `textured` estão byte a byte iguais à fase 11, portanto os caminhos 2D e 3D já
validados não podem regredir por causa de uma luz. `ball_balance` continua em `flat`: os
limiares de cor do CI estão afinados para ele, e a bola é uma esfera vista através de uma
escala uniforme, onde `mat3(u_model)` é exacto.

## Entregue — `ball_balance`, o jogo para medir latência
Data: 2026-11-07

Pedido: um jogo em que se consiga **sentir** a latência e a resposta a comandos —
uma superfície plana suspensa e uma bola que o jogador mantém em cima inclinando a
superfície.

### O que existe

- `giochi-engine-3d/src/game/balance.rs` — a física, **pura e sem GL**. `BalanceGame`
  recebe `(stick_x, stick_z)` e o tempo do quadro; devolve posição e velocidade da
  bola, inclinação da plataforma, queda em curso e pontuação. Por não tocar no
  renderer, os testes correm em CI sem ecrã nenhum.
- `giochi-engine-3d/examples/ball_balance.rs` — o jogo. Plataforma de 6,5 m, bola de
  0,55 m de raio, inclinação máxima de 22°.
- `giochi-engine-3d/src/mesh/primitives.rs::sphere` — a bola, uma esfera UV gerada por
  código; nenhum asset para levar para o console.

### Três indicadores, para separar as fontes de atraso

1. **Marcador de input** (cubo azul) — segue o stick **sem física nenhuma**. Se ele
   atrasa em relação ao polegar, o atraso está no input ou no ecrã, não no jogo.
2. **Plataforma** — inclina a partir do mesmo input, um quadro depois do marcador.
3. **Bola** — soma a simulação aos dois anteriores.

O cubo à esquerda é um indicador de engasgo: verde dentro dos 60 Hz, âmbar a
aproximar-se, vermelho acima. Um stutter vê-se no ecrã sem consola nenhuma.

### Bugs encontrados ao construir isto

1. **Eixo X invertido.** Rodar sobre +Z por um ângulo positivo *levanta* o lado +X,
   portanto rolar para a direita precisa do ângulo **negativo**. A primeira versão
   usava o ângulo positivo e a bola ia para o lado contrário ao stick. O sinal do
   eixo Z já estava certo — a assimetria é o que torna o erro fácil de escrever e
   difícil de ver. Fixado em `tilt_from_input`, com quatro testes a fixar a direção
   (direita, esquerda, perto, e independência dos dois eixos).
2. **Resposta demasiado mole.** Com 12° de inclinação máxima e muito atrito a bola
   andava 0,55 m em 1 s num tabuleiro de 4,5 m de meio-lado: não servia para julgar
   latência, porque nada de mau acontecia. Passou a 22° de inclinação, menos atrito,
   tabuleiro de 6,5 m — a bola fica comprometida em ~2 s de stick a fundo.
3. **`drawable_size` obsoleto.** O exemplo lia o tamanho do `RenderContext3D`, que só
   o aprende dentro de `begin_frame`; na primeira frame recebia o `(1, 1)` inicial e
   o `--quadro` gravava um BMP de 1×1. Passou a perguntar ao `Window`.
4. **Rampa de cor ilegível.** A bola ficava oliva a meio tabuleiro, com uma cor
   próxima do cubo verde do indicador de engasgo. Passou a bola clara que só aquece
   no último terço do caminho até à borda.

### Medido

| Onde | Comando | Resultado |
|---|---|---|
| Mesa 26.0.8 / RX 580, offscreen | `--frames 600 --auto` | 600 frames, sem erro GL, trabalho por quadro p50 0,04 ms / p99 0,09 ms / pior 0,17 ms |
| Física | `--frames 200 --auto` | stick +x → `tilt +0.371`, `v_x +0.04 → +0.30`; bola termina em `(+1.29, +1.75)`, direção correta nos dois eixos |
| Unidade | `cargo test -p giochi-engine-3d --lib game::` | 13 testes, 0 falhas |

`--trace N` imprime uma linha por quadro com input, inclinação, posição e velocidade
da bola. Um input que muda no quadro N e uma bola que reage no quadro N é o correto;
no quadro N+3 é um bug que vale a pena encontrar.

## Entregue na fase 12 — Câmera com controlo + `mini_fps`

O walker em primeira pessoa, e o exemplo jogável que a fase pede.

- `math3d/controller.rs`: `CameraController` com yaw/pitch acumulados (não quaternário
  integrado — evita deriva ao fim de uma sessão), pitch travado aquém dos polos, velocidade
  suavizada e passeio normalizado (W+D não é 41% mais rápido que W).
- `AxisInput`: eixos desacoplados da fonte. Teclado é uma fonte; um caminho
  scriptado é outra. Isto permite testar a matemática de movimento directamente e foi
  o que expôs o bug abaixo.
- `examples/mini_fps.rs`: chão texturizado, 24 cubos com inclinações diferentes, WASD,
  Q/E, sprint com shift, rato relativo com ESC a libertar, TAB para wireframe, FPS no log
  uma vez por segundo, `--frames`/`--auto`/`--quadro`.
- `RenderContext3D::set_wireframe`, `Platform::sdl_mouse`.
- Chaves `KEY_E`, `KEY_Q`, `KEY_LEFT_SHIFT` adicionadas à crate 2D (aditivo).

### Bug encontrado pelo próprio exemplo

O modo `--auto` não movia o jogador. A primeira versão chamava
`AxisInput::from_keyboard`, e `KeyboardState` só é populado por eventos SDL reais — num
percurso scriptado não há teclado, logo os eixos ficavam a zero. O sintoma era traiçoeiro:
o FPS imprimia 60,0 e o programa saía com sucesso, mas a posição não mudava. Resolvido com
`update_with_axes` e um teste que assere a **direção** de cada eixo (W → −Z, D → +X,
E → +Y) e que a diagonal não é mais rápida que a linha reta.

### Medido no PX30 (Mali-G31/Panfrost), 2026-11-07 13:40

| Verificação | Resultado |
|---|---|
| `mini_fps --frames 300 --auto` | **60,0 fps** em todos os relatórios, 0 erros GL |
| `mini_fps --frames 1200 --auto` (sem cap) | **57,1 fps** (21 s) |
| Movimento | (0,1.7,8) → (38.2,1.7,−101.0) em 1200 frames |
| Captura no aparelho | BMP 640×480; céu em cima, chão em baixo, cubos com profundidade |
| Libertação | `released: 0 meshes and 0 shaders left` |
| FPS no desktop (`offscreen`) | 60,0 com cap, mesma cena |

**Validado em hardware real.** `--auto` existe precisamente porque "jogável" não é
testável automaticamente: o caminho manual (rato relativo, WASD, TAB, ESC) usa o mesmo
código de movimento, mas o que se pode medir — direção, colisão de profundidade, fps — é
medido.

## Entregue na fase 11 — Pipeline de mesh + shaders

Primeira geometria 3D real na tela: VBO/EBO, shaders GLSL ES 1.00, depth buffer.

- `mesh/vertex.rs`: `Vertex` `#[repr(C)]` + `Pod`, 32 bytes, interleaved
  `position/normal/uv`, upload sem passe de conversão.
- `mesh/gpu_mesh.rs`: `Mesh` com VBO, EBO e **VAO opcional**. `glGenVertexArrays` é
  core só no GLES 3.0; no 2.0 depende de `GL_OES_vertex_array_object`. Sem a extensão,
  os atributos são reespecificados a cada draw — o chamador não vê diferença.
- `shader/program.rs`: compilação com log completo do driver (`glGetShaderInfoLog` /
  `glGetProgramInfoLog`) mais o source; uniformes com cache por nome via `RefCell`.
- `shader/builtin.rs` + `shaders/*.vert|frag`: `flat` e `textured`, `include_str!`ed.
- `mesh/obj_loader.rs`: OBJ via `tobj`, interleaving, normais derivadas quando o ficheiro
  não as traz, múltiplos objectos concatenados com índices rebaseados.
- `mesh/primitives.rs`: `cube` e `plane` procedurais — o exemplo não depende de asset
  binário.
- `render3d/renderer.rs`: `create_mesh`/`destroy_mesh`, `create_builtin_shader`/
  `load_shader`/`destroy_shader`, `draw_mesh`, `release_all`, contadores.
- `examples/cube_demo.rs`: cubo a girar em dois eixos, `--frames`, `--quadro`.

### Detalhe que faz o desenho funcionar

Os slots de atributo são fixados com `glBindAttribLocation` **antes** do link
(`a_position`=0, `a_normal`=1, `a_uv`=2). Sem isso o GLES 2.0 — que não tem
`layout(location=)`) — atribui índices diferentes por programa, e um VAO por mesh não
poderia servir mais de um shader.

### Bugs encontrados pelos testes

1. **Duas faces do cubo com winding invertido** (+X e −X). O teste que verifica só a
   primeira face passava; o que verifica as seis encontrou as duas. Com culling activo o
   cubo parecia oco.
2. **Underflow no loader OBJ**: a passagem de normais percorria a lista de índices
   inteira, então o segundo objecto subtraía um `base` maior que os próprios índices e
   entrava em pânico em debug / corrompia em release.
3. **Teste de sintaxe GLES 3 com falso positivo**: procurava `"out "` e encontrava a
   palavra inglesa "without" num comentário. Agora remove comentários primeiro.

### Medido (GL real, Mesa 26.0.8 / AMD RX 580, `SDL_VIDEODRIVER=offscreen`)

| Verificação | Resultado |
|---|---|
| `cargo test -p giochi-engine-3d` | **138 testes** (106 lib + 9 layout + 3 contexto + 7 matemática + 8 pipeline + 5 doc) |
| Shaders `flat` e `textured` num driver real | compilam |
| Shader inválido | erro com o log do driver e o source |
| Cubo rasterizado | **97 147 pixels** acesos, bbox 313×314 centrada em (643, 357) em 1280×720 |
| Cubo atrás da câmara | **0 pixels** acesos |
| Caminho sem VAO (`GIOCHI_FORCE_NO_VAO=1`) | mesmos pixels que o caminho com VAO |
| `release_all` | 0 meshes, 0 shaders |
| Captura `--quadro` | BMP 1280×720, 3,7 MB, quadrado sólido centrado |
| Build cruzado aarch64 `--all-targets` | linka, `cube_demo` é ELF ARM aarch64 |

**Validado no PX30 (Mali-G31/Panfrost, Mesa 24.0.0), 2026-11-07 12:30.** O teste manual
que a fase exigia foi feito, e a captura de quadro prova que desenhou:

| Verificação | Resultado |
|---|---|
| `cube_demo --frames 300` (GLES 2 padrão) | 300 frames, **0 erros GL**, VAO disponível |
| `GIOCHI_FORCE_NO_VAO=1 --frames 300` | 300 frames, 0 erros GL, caminho de fallback |
| `GIOCHI_GL_VERSION=3 --frames 200` | GLES 3, 200 frames, 0 erros GL |
| `--frames 1200` | **57,1 fps** (21 s) |
| Captura `--quadro` **no aparelho** | BMP 640×480; cubo 209×209 px, 43 162 pixels acesos, centrado em (322,238) com o painel em (320,240) |
| `--obj ../assets/tri.obj` | 3 vértices carregados, 60 frames, 0 erros GL |
| `--obj` inexistente | `asset error: OBJ file does not exist: ...` |
| Memória residente | **19,9 MB** RSS (VmPeak 180 MB) |
| Libertação | `released: 0 meshes and 0 shaders left` |

O cubo centrado no painel 4:3 é a prova de que a fase 10 também está certa no hardware:
o aspect vem do drawable real, não de um 16:9 assumido.

## Entregue na fase 10 — Math 3D

Cinco tipos `#[repr(C)]` com a aritmética em `glam`, que nunca aparece em campo
público.

- `math3d/vec3.rs`, `quat.rs`: `Vec3` (28 bytes) e `Quat` (32 bytes), com
  `normalized()` que devolve zero/identidade em vez de NaN para entradas degeneradas —
  um quatérnio zero vindo do C blankaria o frame sem erro nenhum.
- `math3d/mat4.rs`: `Mat4` coluna-maior, 80 bytes, `as_ptr()` pronto para
  `glUniformMatrix4fv(transpose = GL_FALSE)`. `perspective`, `orthographic`, `look_at`
  devolvem `Err` para frustum degenerado em vez de produzir uma matriz que não desenha.
- `math3d/transform.rs`: `Transform3D` (104 bytes), composição
  **escala → rotação → translação**.
- `math3d/camera.rs`: `Camera3D` (116 bytes), `aspect == 0.0` significa "não resolvido"
  e `projection_matrix()` devolve erro em vez de adivinhar 16:9.
- `RenderContext3D::begin_frame(&window)` agora **define o viewport** a partir do
  drawable real, e expõe `drawable_size()`/`aspect()` para a câmera. Antes o viewport
  nunca era configurado — no KMSDRM, onde o drawable real difere do pedido (ADR-035),
  isso desenharia no canto do painel.
- `tests/math3d_integration.rs`: 7 testes de composição entre tipos.
- `tests/abi_layout_3d.rs` reescrito para usar os tipos reais: 9 testes que travam
  28/32/32/104/116/80 bytes e provam que nenhum tipo SIMD vazou para a superfície.

### Bug encontrado pelos próprios testes

`glam::perspective_rh` mapeia profundidade em **0..1** (Vulkan/D3D); OpenGL ES espera
**-1..1**. A primeira versão usava a errada: toda matriz parecia plausível, nenhum erro
GL, e a geometria só quebraria perto do near plane. O teste que assere o NDC z no near
plane pegou. Corrigido para `perspective_rh_gl`/`orthographic_rh_gl`, com o intervalo
travado por teste.

### Medido

| Verificação | Resultado |
|---|---|
| `cargo test -p giochi-engine-3d` | **95 testes** (75 lib + 9 layout + 3 GL + 7 integração + 1 doc) |
| `SDL_VIDEODRIVER=offscreen --frames 200` | 200 frames, nenhum erro GL, câmera resolvida por frame |
| GLES 2 pedido com driver a reportar 3.2 | reporta **GLES 2** (a correção da fase 9 vale no desktop) |
| `cargo check --target aarch64` | passa |
| Build cruzado do crate + exemplo para aarch64 | linka, ELF ARM aarch64 |

**Validado no PX30** junto com a fase 11: a câmera resolve o aspect pelo drawable real e
o cubo sai centrado no painel 640×480 (bbox centrada em (322,238), painel em (320,240)).
Um aspect assumido como 16:9 teria deslocado ou cortado o cubo.

## Entregue na fase 9 — Bootstrap 3D (contexto GLES/EGL)

Crate nova `giochi-engine-3d`, paralela ao 2D (ADR-037). Não toca em `sdl2::render`.

- `context/gl_context.rs`: `configure_attributes` (atributos `SDL_GL_*`, que são
  globais do processo e lidos na criação da **janela**), `create` (contexto via
  `SDL_GL_CreateContext` + `glow::Context::from_loader_function`), `swap`,
  `GlesVersion` com detecção pelo string do driver e `GlesVersion::from_env`.
- `context/state.rs`: `GlState` explícito por frame (depth test, cull, front face,
  blend, cor de clear). Regra de ouro 6 da extensão: nada é assumido.
- `render3d/renderer.rs`: `RenderContext3D` com `begin_frame`/`end_frame`,
  `take_gl_error` e `read_pixels` (RGBA8 via `glReadPixels`).
- `math3d/color.rs`: `Color3D` `#[repr(C)]`, 32 bytes, clamp incluindo NaN.
- `examples/cube_demo.rs`: tela azul, fecha com ESC, `--frames N` para execução
  não-interativa (CI e hardware).
- `docs/C_ABI_CONTRACT.md` parte II (§8–§12): tipos 3D completos, versão ABI 2
  especificada, funções da fase 14 declaradas. Escrito **antes** do código, como R11
  exige.
- `docs/3D.md`: guia da fase, tabela de drivers GL, troubleshooting, estado por fase.
- ADR-037 (crate separada, não backend), ADR-038 (GLES 2.0 padrão, 3.0 opt-in),
  ADR-039 (ponteiro opaco de engine no 3D). ADR-005 anotada como superada no 3D.
- Adições **retrocompatíveis** na crate 2D: `Window::new_opengl`, `Window::sdl_window`,
  `Platform::video_subsystem`. Nenhuma assinatura existente mudou.

### Medido

| Verificação | Resultado |
|---|---|
| `SDL_VIDEODRIVER=offscreen` + `--frames 120` | 120 frames, **nenhum erro GL** |
| `SDL_VIDEODRIVER=offscreen` + `GIOCHI_GL_VERSION=3` | contexto GLES 3.0 (Mesa 26.0.8, AMD RX 580) |
| `cargo test -p giochi-engine-3d --lib` | 17 testes |
| `--test context_smoke` (GL real) | 3 testes, incluindo leitura do pixel de clear |
| `--test abi_layout_3d` | 5 testes de layout |
| `SDL_VIDEODRIVER=dummy` | falha descritiva: o driver não tem OpenGL |
| `cargo check --target aarch64` | passa |
| **PX30 real, `--frames 300` (padrão GLES 2)** | `GLES 2 on Mali-G31 (Panfrost)`, 300 frames, **0 erros GL** |
| **PX30 real, `GIOCHI_GL_VERSION=3 --frames 300`** | `GLES 3 on Mali-G31`, 300 frames, 0 erros GL |
| **PX30 real, `--frames 1200`** | 20,57 s = **58,3 fps** |

**Validado em hardware real.** O teste manual que a fase exigia foi feito: a tela
abre, o clear funciona e o swap apresenta no Mali-G31 via KMSDRM, sem erro de GL.

### Bug encontrado no hardware (e corrigido)

Pedir `SDL_GL_CONTEXT_MAJOR_VERSION=2` no KMSDRM do PX30 produzia um contexto
**OpenGL ES 3.1** de qualquer forma — o SDL2 honra o profile mas não a versão nesse
driver. A primeira versão reportava o que o driver dizia, o que quebrava a promessa de
`supports_gles3()`: um jogo que pediu o piso 2.0 podia ver um caminho 3.0 ativar.
Corrigido: **pedido 2.0 → reportado 2.0 sempre**; pedido 3.0 → 3.0 só se o driver
confirmar, senão 2.0 com aviso. Detalhes em `docs/3D.md` §3 e `docs/KMSDRM.md` §8.8.

### Notas de fase

- O driver `dummy` do SDL **não tem GL**; `offscreen` tem a pilha completa. Isso
  decidiu como o CI testa 3D sem GPU: `SDL_VIDEODRIVER=offscreen`.
- `glReadPixels` no CI é o que separa "a chamada foi feita" de "o pixel é aquela
  cor" — a mesma distinção que o `--quadro` faz no 2D.
- Os tamanhos de struct do contrato foram corrigidos contra um compilador C real:
  o primeiro rascunho dizia 92 e 64 para `Transform3D` e `Camera3D`; os valores certos
  são **104** e **116**.

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

**Estado no GitHub (execuções reais, até `6b074b0`):**

| Workflow | Jobs | Resultado |
|---|---|---|
| `ci.yml` | gates, ffi, cross-check, smoke | ✅ verde |
| `cross-compile.yml` | check, link (`ubuntu-24.04`) | ✅ verde, link ARM64 real |
| `release.yml` | build-x86_64, build-aarch64, publish | ✅ verde; execução `42384430385` a partir da tag `v0.1.1` |

Validação: a matriz completa de CI espelhada num contentor `ubuntu:24.04` limpo — fmt,
clippy, build, 209 testes, doc, header, FFI (C + Python), demo, os cinco exemplos e o
`mini_game.lua` — **16/16 passos**, 0 falhas.

## Verificação do artefato `v0.1.1` (2026-11-07)

O workflow cria a release em **rascunho** de propósito (ver `CONTRIBUTING.md`,
"Releasing"): os arquivos só são publicados depois de alguém os executar. O ensaio
abaixo foi feito exatamente para essa revisão, a partir do `.zip` que o
`upload-artifact` gerou na execução `42384430385` — não de uma build local:

```
giochi-engine-x86_64-unknown-linux-gnu.tar.gz   8 578 531 bytes
giochi-engine-aarch64-unknown-linux-gnu.tar.gz  8 430 137 bytes (artefato)
```

Conteúdo do tarball x86_64: `giochi-demo`, `include/giochi.h`, `lib/libgiochi.{so,a}`,
`lua/mini_game.lua`, os oito documentos, `README`/`LICENSE`/`CHANGELOG`/`PROGRESS` e
`assets/` com **os dois formatos** (`player.png`/`player.bmp`, `checker.png`/`checker.bmp`).

Executado depois de extrair, com `SDL_VIDEODRIVER=dummy`:

| Verificação | Resultado |
|---|---|
| `giochi.h` diz a versão | `For version \`0.1.1\` this returns \`0x0000_0101\`` |
| Símbolos `giochi_*` exportados em `libgiochi.so` | 57 |
| `giochi-demo --verify` | 1 textura viva, áudio ativo |
| `giochi-demo lua lua/mini_game.lua --auto --frames=60` | 0 texturas vivas no fim |
| `c_smoke` compilado contra o `include/` e o `lib/` do tarball | `OK (abi 1, engine 0.1.1)` |
| Os mesmos três, **apagando os PNG** (só BMP, como no console) | todos passam |

O último item é o que valida a ADR-036 de ponta a ponta: o artefato publicado continua
a funcionar num alvo sem decoder PNG.


## Validação em hardware real (2026-11-07)

Primeira execução numa consola de verdade: **Rockchip PX30** portátil, Buildroot
2024.02, kernel 6.12, aarch64, painel DSI 640x480, glibc 2.38, sem X11 e sem Wayland,
964 MB de RAM, CPU ARMv8.

Quatro bugs de portabilidade que nem o CI nem a suíte de desktop podiam ter apanhado, mais
uma lacuna que o próprio teste revelou — a do item 5, que só aparece quando se tenta
correr o *repositório* no aparelho em vez de um binário preparado à mão:

1. **`libSDL2_image` sem codecs.** Nessa imagem a biblioteca liga apenas `libSDL2`,
   `libm` e `libc`: PNG e JPEG são stubs "not supported". Como `IMG_Init` falha quando
   *qualquer* flag pedida está indisponível, pedir `PNG | JPG` fazia a engine declarar
   todo o subsistema morto — apesar de BMP, TGA, PNM e QOI não precisarem de flag
   nenhuma. Agora abre-se sem flags e cada codec opcional é sondado em separado, com um
   aviso por formato em falta (ADR-034).
2. **KMSDRM é fullscreen-only e o log mentia.** Pedir 1280x720 num painel de 640x480
   abre 640x480, sem erro e sem escala; o arranque imprimia o tamanho *configurado*.
   Agora reporta o drawable real e escreve `requested WxH, backend opened WxH` quando
   diferem (ADR-035).
3. **O nome do driver é `KMSDRM`, em maiúsculas** (`x11` e `wayland` são minúsculas), pelo
   que o natural `strcmp(driver, "kmsdrm")` nunca casava numa consola — e o erro era
   invisível no desktop. Documentado; o smoke test C compara sem distinguir maiúsculas.
4. **O cross ARM64 com sysroot vazava pacotes do host.** Duas omissões independentes: o
   toolchain CMake definia `CMAKE_SYSROOT` mas não `CMAKE_FIND_ROOT_PATH`, e o pkg-config
   continuava a responder com `.pc` x86_64 do host, pelo que o SDL2 activava
   X11/Wayland/EGL/PulseAudio/sndio e o linker aarch64 recebia sonames do host. Ambos
   corrigidos (`cmake/aarch64-linux-toolchain.cmake`, `scripts/build-aarch64.sh`).
5. **Os assets do repositório não corriam no alvo.** `assets/` só tinha PNG, e nessa
   imagem não há decoder PNG: um `git clone` fresco não conseguia correr o próprio demo
   no aparelho. O teste em hardware só passou porque um BMP convertido à mão foi
   enviado **com o nome `.png`**, que a auto-detecção de formato aceitava por acidente.
   Agora cada asset existe em duas versões, PNG e BMP, com os mesmos pixels, e o demo e
   o `mini_game.lua` caem para o BMP quando o PNG falha (ADR-036). O
   `scripts/png-to-bmp.py` regenera os pares e o `tests/asset_formats.rs` falha o CI
   quando um deles fica desactualizado.

Duas armadilhas de operação, ambas registadas em `docs/KMSDRM.md` §8:

- a consola arranca no seu próprio launcher, que detém `/dev/dri/card0` durante toda a
  vida — a mensagem "outro processo detém DRM master" **não** significa só "pare o X".
  Libertar exige tornar o launcher não-executável (o supervisor respeita `[ -x ]`) e
  depois sinalizar `giochi-init`;
- o supervisor desiste em definitivo após 5 falhas em 60 s e estaciona num
  `sleep 3600`: um binário de teste que rebente cinco vezes deixa o ecrã preto sem
  recuperação a não ser sinalizar `giochi-init`.

**Medições nesse dispositivo** (build ARM64 de release, 640x480, vsync desligado):

| Medição | Resultado |
|---|---|
| `giochi-demo --frames 300` | 59,4 fps, 303 passos |
| `giochi-demo lua mini_game.lua --auto --frames 1200` | 59,5 fps, 16 pontos, **0 texturas vivas** |
| Memória residente (`VmRSS`) | 21,9 MB (`VmPeak` 181 MB) |
| Consumidor C (`c_smoke`) | `OK (abi 1, engine 0.1.x)` |
| Áudio | disponível (ALSA, `/dev/snd/pcmC0D0p`) |
| Gamepad | detetado (`r36s_Gamepad`) |

O regresso a zero texturas depois de 1200 frames de jogo é a prova de não-fuga: o binding
Lua liberta texturas no `Drop` e o registo confirma que nada ficou para trás. A build
ARM64 exige no máximo `GLIBC_2.35`, compatível com os 2.38 do dispositivo.

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

- **Fases 0–15 concluídas.** O plano mestre (0–14) está completo; a extensão de
  performance já entregou a fase 15 e tem as fases 16–18 por fazer.
- **Fase 15 validada no PX30 (2026-11-08).** O aparelho respondeu por SSH; a baseline
  2D a 640x480, os contadores 3D e o `mini_fps` foram medidos com o launcher
  desativado/restaurado e o binário em `/tmp`. Ver `docs/PERF_LOG.md`.
- **O aparelho em `10.55.0.2` lê bateria a 0 % / 0,018 V**, mas está a correr do
  carregador (`online=1`). A campanha de testes completa correu nesse estado a pedido do
  utilizador, sem incidente: os três binários 3D e os dois consumidores C terminaram com
  código 0. A leitura de 0,018 V não é uma tensão de célula — é o que o `rk817` reporta
  quando não consegue medir. Tratar como **sem bateria, apenas com a fonte**: desligar o
  cabo durante uma sessão continua a ser o cenário que apaga o rootfs.
- ~~C ABI 3D validada em x86_64 mas ainda não no PX30.~~ **Resolvido:** `c_smoke` e
  `c_smoke_3d` correram no Mali-G31 (ver a secção de validação acima), a par do demo, do
  jogo Lua, dos três exemplos 3D e do `ball_balance`.
- Nenhum bloqueio de gate.
- **`ball_balance` está no aparelho e validado no Mali-G31.** Os três exemplos 3D
  estão em `/opt/giochi/bin/` e correram no hardware: `cube_demo`, `mini_fps` (com
  Phong) e `ball_balance` (240 frames, 0 erros GL, p50 1,07 ms). Ver a secção de
  validação acima.
  **Cuidado ao repetir:** a bateria estava a 8 % com o carregador `online=0` (o
  USB-ethernet dá dados e não carrega). O aparelho esteve 55 min estável a 3,75 V, o
  que chega para um deploy, mas o rootfs **não sobrevive a um corte de energia**. Por
  isso `scripts/hardware-check.sh` passou a ler a bateria **antes** de escrever na
  flash e recusa correr abaixo de 5 % ou abaixo do `voltage_min_design`; há um
  `--force-low-battery` para quando for uma decisão consciente. A sequência completa é
  `scripts/hardware-check.sh --host root@10.55.0.2 --sysroot /tmp/r35s-sysroot
  --skip-build`.
- **PX30 acessível de novo (13:30).** O acesso voltou; fases 10 e 11 validadas no
  Mali-G31 (ver as secções das fases). Nota operacional: o aparelho tinha perdido
  `/opt/giochi` e o `authorized_keys`, ou seja o rootfs não sobrevive a um corte de
  energia — o deploy precisa de ser refeito a cada reset. O *registro* histórico segue
  abaixo.
- **PX30 inacessível (2026-11-07, tarde) — confirmado de novo às 13:00.** O adaptador
  USB-ethernet voltou (`enx0247494f4348`, 10.55.0.1/24) e o aparelho responde a ping,
  mas `ssh` oferece a chave e o servidor **rejeita**: `Permission denied (publickey,
  password, keyboard-interactive)`. A chave de host também mudou
  (`SHA256:dxsN4ork...` agora). Só a porta 22 está aberta; não há consola série (o
  adaptador CH341 desapareceu do `lsusb`). Recuperação exige acesso físico.
  Contexto original: Depois de a bateria esgotar e voltar, o
  aparelho regenerou a chave de host SSH e perdeu as chaves autorizadas:
  `Permission denied (publickey,password,keyboard-interactive)`. Antes disso a mesma
  unidade tinha aceitado a chave sem intervenção, então isto indica reflash do rootfs
  (ou um rootfs em tmpfs que não sobreviveu ao corte de energia). Consequência: a
  fase 10 está validada em GL real no desktop (`SDL_VIDEODRIVER=offscreen`, Mesa/EGL)
  mas **não no Mali-G31**. Recuperar exige acesso físico: reinstalar a chave pública
  em `/root/.ssh/authorized_keys` pela consola série, ou reflashar. A fase 9 tinha sido
  validada no hardware antes da queda e continua registada acima.
- CMake 4.2.3 × `cmake_minimum_required(VERSION 3.0.x)` do SDL2 vendorizado:
  workaround em `.cargo/config.toml` (ADR-014).
- `sdl2` compila com `unsafe_textures` (ADR-017); um único canvas por processo
  (ADR-018).
- O caminho "ao vivo" do gamepad não é exercitável sem hardware (ADR-020).
- Nesta máquina de desenvolvimento o `kmsdrm` não inicializa (sessão gráfica detém DRM
  master) — caso coberto pelo fallback do ADR-015. O caminho KMSDRM real está agora
  **provado em hardware** (PX30, secção acima); o mixer também foi verificado com placa
  de som real.
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
