# giochi-editor — guia do utilizador

Editor visual de cenas da GIOCHI-ENGINE. Este documento cobre as **Fases 30 a 32**: como
abrir o editor, o que já está na janela, como navegar no viewport, como criar/abrir/gravar
cenas e como selecionar objetos. Cresce uma secção por fase até ao guia completo da
Fase 40.

---

## 1. Abrir

```bash
cargo run -p giochi-editor --release
cargo run -p giochi-editor --release -- --scene giochi-editor/assets/scenes/default.scene
```

A janela abre a **1600×900**, redimensionável. Sem ecrã (CI, SSH, KMSDRM sem TTY) o SDL
recusa o driver e o editor termina com a mensagem do `giochi-engine`; nesse caso corre-se
o verificador sem janela:

```bash
SDL_VIDEODRIVER=dummy cargo run -p giochi-editor -- --frames 3
# INFO giochi_editor: giochi-editor v0.1.0
# INFO giochi_editor: giochi-editor: 3 frame(s) rendered
```

`--frames N` desenha N frames e sai. É o que o teste `tests/editor_smoke.rs` usa e o que
torna a aceitação da fase verificável sem clicar. `--scene FILE` abre uma cena antes do
primeiro frame; sem ela, o editor começa vazio e diz como criar uma.

### Drivers de vídeo

O `giochi-engine` tenta **KMSDRM** primeiro, porque é o alvo do GIOCHI-OS; num desktop
isso falha com um aviso e cai para o driver da sessão (`x11`/`wayland`). O aviso é
esperado. Para o silenciar:

```bash
SDL_VIDEODRIVER=x11 cargo run -p giochi-editor
```

---

## 2. O que está na janela

```text
┌──────────────────────────────────────────────────────┐
│ File  Edit  View  Play  Help                         │
├────────────┬────────────────────────────┬────────────┤
│ HIERARCHY  │                            │ INSPECTOR  │
│            │         VIEWPORT           │            │
│            │                            │            │
├────────────┴────────────────────────────┴────────────┤
│ ASSET BROWSER                                        │
└──────────────────────────────────────────────────────┘
```

Os quatro painéis e a barra de menus estão lá desde a Fase 30. **O conteúdo deles chega
depois**: cada painel diz qual é a fase que o preenche.

| Painel | Conteúdo | Fase |
|---|---|---|
| Hierarchy | árvore com hierarquia, ícones por tipo, seleção | **32, feito** |
| Inspector | Transform, Mesh, Material, Light, Camera | 33 |
| Asset Browser | árvore de assets do projeto, import, drag-to-viewport | 34 |
| Viewport | cena em wireframe, grid, eixos, câmara orbit | **31, feito** |

O viewport mostra o **mundo**, não o sombreado: grid no plano XZ, os três eixos, a caixa de
cada nó (malha, luz, câmara, grupo) e o contorno amarelo do selecionado. É deliberado — a
Fase 31 não abre um segundo contexto GL dentro do editor (ADR-064); o jogo é que mostra o
resultado com materiais e luz.

### Menu

`File`, `Edit`, `View`, `Play` e `Help` abrem. As entradas que ainda não existem estão
**desativadas** e, ao passar o rato, dizem em que fase chegam — não desaparecem, para o
mapa de menus não mudar de fase para fase.

Já funcionam:

| Ação | Onde | Atalho |
|---|---|---|
| Nova cena (cubo + sol + câmara) | `File > New Scene` | `Ctrl+N` |
| Abrir cena | `File > Open Scene…` | `Ctrl+O` |
| Gravar | `File > Save` | `Ctrl+S` |
| Gravar como | `File > Save As…` | `Ctrl+Shift+S` |
| Enquadrar a cena | `View > Frame Scene` | — |
| Mostrar/esconder painéis | `View` | — |
| Sair | `File > Exit` | `Esc` |

Esconder um painel devolve o espaço ao viewport; o layout do egui é elástico, por isso
arrastar as bordas dos painéis também funciona.

O menu bar mostra à direita a última ação ("Opened …", "Saved …") ou o erro que a impediu.
Substituir uma cena com alterações por gravar é recusado: grave primeiro.

## 3. Teclado

| Tecla | Efeito |
|---|---|
| `Ctrl+N` / `Ctrl+O` / `Ctrl+S` / `Ctrl+Shift+S` | nova, abrir, gravar, gravar como |
| `Esc` | Sair — exceto quando a consola, um diálogo ou um campo de texto tem o foco |
| `F2` | Abrir/fechar a consola in-game do `giochi-engine` |

`Q`/`W`/`E`/`R` (select/translate/rotate/scale) chegam com os gizmos, nas Fases 35a/35b;
`Ctrl+Z`/`Ctrl+Y` chegam na Fase 37. O estado `EditorMode` já existe para os receber.

---

## 3.1 Hierarchy e seleção

A árvore mostra a hierarquia da cena (nós filhos dentro do pai, com `CollapsingHeader`),
com um ícone por tipo (`mesh`, `light`, `camera`, `group`). Um objeto selecionado fica
destacado na árvore e com contorno amarelo no viewport.

| Gesto | Onde | Efeito |
|---|---|---|
| click | árvore ou viewport | seleciona **só** esse objeto |
| `Ctrl`+click | árvore ou viewport | adiciona/remove da seleção (multi-seleção) |
| duplo-click | árvore ou viewport | seleciona **e** enquadra o objeto no viewport |
| click no vazio | viewport | limpa a seleção |

A escolha no viewport é por **raycast** contra a caixa (AABB) de cada nó — o mesmo
`viewport::node_world_bounds` que desenha as caixas, por isso o objeto que se clica é o que
se vê. O mais próximo ao longo do raio ganha. A seleção é guardada por id estável
(`SceneNodeId`), o mesmo id que o ficheiro da cena usa.

`View > Frame Scene` enquadra a cena inteira; o duplo-click enquadra um objeto.

## 3.2 Viewport

| Gesto | Efeito |
|---|---|
| arrastar com o **botão direito** | orbitar em torno do alvo |
| arrastar com o **botão do meio** | fazer pan no plano do ecrã |
| **roda** do rato | zoom multiplicativo (um entalhe muda a mesma proporção a qualquer distância) |
| `View > Frame Scene` | enquadra o AABB de todas as malhas |

A câmara do viewport é **independente** da câmara da cena: mover-se no editor não altera o
que o jogo verá. O canto inferior esquerdo mostra a distância atual.

Ao abrir ou criar uma cena, a câmara enquadra automaticamente as malhas existentes.

## 3.3 O diálogo de ficheiros

`File > Open Scene…` e `File > Save As…` abrem um diálogo desenhado pelo próprio editor
(egui), não o `rfd`: no GIOCHI-OS não há `xdg-desktop-portal` nem D-Bus de sessão, e um
diálogo que só funciona no desktop do programador é pior do que nenhum. O diálogo lista
pastas e ficheiros `.scene`, tem campo de nome, `Up` e `Refresh`. ADR-064 regista a troca.

---

## 4. A consola (`F2`)

Reutiliza a consola da Fase 20 tal e qual: os mesmos comandos (`help`, `fps`, `entities`,
`clear`, `spawn`, …), o mesmo histórico e o mesmo autocomplete. É o painel inferior
`TopBottomPanel` que a Fase 20 já desenha; no editor serve para inspecionar o processo sem
sair dele.

`Esc` com a consola aberta fecha **a consola**, não o editor: a consola vê as teclas
primeiro e o evento que ela consome não chega ao ciclo do editor.

---

## 5. Arquitetura em uma página

O editor **consome** a engine. O ciclo é dele, não do `Engine::run_current`, porque um
editor não tem passo fixo nem pilha de cenas:

```text
poll eventos SDL
  → devtools (F2, teclas da consola)
  → tradução para egui
  → clear
  → backend egui: tema → layout → consola
  → paint
  → present
```

A janela, o renderer, o mixer e a consola continuam a ser os da engine. O único ponto
estrutural que faltava era poder correr um frame egui a partir de um ciclo externo; o
`EguiBackend` da Fase 21 já fazia isso e passou a ser público (ADR-062). Não há código
de pintura duplicado no editor.

| Módulo | Responsabilidade |
|---|---|
| `src/lib.rs` | opções, ciclo, `run()`, atalhos |
| `src/main.rs` | linha de comando (`--scene`, `--frames`, `--help`) |
| `editor/editor_state.rs` | cena aberta, seleção, modo, dirty, painéis, gravar/abrir |
| `editor/layout.rs` | painéis, menu, viewport; devolve `LayoutMetrics` |
| `editor/orbit_camera.rs` | câmara do viewport (yaw/pitch/distância, pan, zoom, projeção) |
| `editor/viewport.rs` | projeção e desenho do wireframe, clipping ao retângulo |
| `editor/dialog.rs` | diálogo de abrir/gravar desenhado em egui |
| `editor/theme.rs` | paleta e escala tipográfica do editor |

O desenho do viewport acontece **antes** do egui, com o retângulo que o layout produziu no
frame anterior: assim uma linha projetada uns pixels fora do viewport não aparece por cima
de um painel.

---

## 6. Pendências conhecidas (fim da Fase 32)

| Pendência | Fase |
|---|---|
| Inspector editável | 33 |
| Asset browser, drag-to-viewport | 34 |
| Gizmos translate/rotate/scale | 35a/35b |
| Criar/apagar/duplicar/reparentar | 36 |
| Undo/redo global | 37 |
| Projetos, `recent.toml`, autosave, aviso ao fechar, metadados do editor no save | 38 |
| Play-in-editor | 39 |
| Preferências, About, atalhos no menu | 40 |
| Sombreado no viewport (FBO/GLES) | fora de âmbito — ADR-064 |

---

## 7. Problemas comuns

| Sintoma | Causa | Solução |
|---|---|---|
| `failed to create window` com `SDL_VIDEODRIVER=kmsdrm` | não há TTY/DRM na sessão | deixar o SDL escolher, ou `SDL_VIDEODRIVER=x11` |
| aviso `kmsdrm video driver unavailable` | comportamento normal num desktop | ignorar ou fixar `SDL_VIDEODRIVER` |
| janela abre e fecha logo | `--frames` foi passado | correr sem `--frames` |
| texto pequeno em HiDPI | o backend usa `pixels_per_point = 1.0` | pendente (Fase 40, preferências) |
| "unsaved changes — Save before replacing the scene" | há alterações por gravar | `Ctrl+S`, depois abrir/criar |
| a cena abre mas o viewport está vazio | a câmara aponta para o lado oposto | `View > Frame Scene` |
