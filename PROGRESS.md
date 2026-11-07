# Estado — GIOCHI-ENGINE

## Última fase concluída: 12 — Câmera com controlo + mini_fps
Data: 2026-11-07

**Fases 0–8 (2D) concluídas. Extensão v2.1 (3D) em andamento: fases 9 a 12 concluídas.**

## Status 2D (Fases 0–8)

Todas concluídas — nenhuma regressão tolerada pelas fases 3D (regra de ouro da
extensão v2.1). Gates 2D revalidados a cada fase 3D.

## Status 3D (Fases 9–14)

- [x] Fase 9 — Bootstrap 3D: contexto GLES/EGL, clear, swap
- [x] Fase 10 — Math 3D (`Vec3`, `Quat`, `Mat4`, `Transform3D`, `Camera3D`)
- [x] Fase 11 — Pipeline de mesh + shaders
- [x] Fase 12 — Câmera com controlo + `mini_fps`
- [ ] Fase 13 — Iluminação Phong (opcional)
- [ ] Fase 14 — C ABI 3D (v2)

## Gates validados

Comando único: `scripts/gates.sh` (`SKIP_ARM64=1` pula o cross-check).

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace --target x86_64-unknown-linux-gnu`
- [x] `cargo test --workspace --target x86_64-unknown-linux-gnu` — 360 testes, 0 falhas
      (209 das fases 0–8 + 151 da crate 3D)
- [x] `cargo check --workspace --target aarch64-unknown-linux-gnu`
- [x] `cargo doc --workspace --no-deps --document-private-items` (0 warnings)
- [x] `scripts/gen-header.sh --check` — `giochi.h` em sincronia com o crate
- [x] `scripts/ffi-smoke.sh` — consumidores C e Python (fase 7, mantidos verdes)
- [x] `SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test context_smoke`
      — 3 testes contra GLES real (Mesa/EGL), incluindo leitura de pixel

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

- Nenhum bloqueio de gate.
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
