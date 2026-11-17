# giochi-editor — guia do utilizador

Editor visual de cenas da GIOCHI-ENGINE. Este documento cobre as **Fases 30 a 34**: como
abrir o editor, o que já está na janela, como navegar no viewport, como criar/abrir/gravar
cenas, como selecionar e editar objetos, e como trabalhar com projetos e assets. Cresce uma
secção por fase até ao guia completo da Fase 40.

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
| Inspector | Transform, Mesh, Material, Light, Camera | **33, feito** |
| Asset Browser | assets do projeto, filtros, preview, import, drag-to-viewport | **34, feito** |
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
| Novo projeto | `File > New Project…` | — |
| Abrir projeto | `File > Open Project…` | — |
| Importar asset | `File > Import Asset…` | `Ctrl+I` |
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
| `Ctrl+I` | Importar um asset para o projeto |
| `Q` / `W` / `E` / `R` | select / translate / rotate / scale |
| `Ctrl+D` | duplicar a seleção |
| `Del` / `Backspace` | apagar a seleção |
| `Ctrl+Z` | desfazer |
| `Ctrl+Y` / `Ctrl+Shift+Z` | refazer |
| `F2` | Abrir/fechar a consola in-game do `giochi-engine` |


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

### Editar a árvore (Fase 36)

| Ação | Como |
|---|---|
| criar | o botão `+` no topo do painel, ou `Edit > Create` |
| duplicar | `Ctrl+D`, `Edit > Duplicate`, ou o menu de contexto da linha |
| apagar | `Del`/`Backspace`, `Edit > Delete`, ou o menu de contexto |
| mudar de pai | arrastar uma linha para cima de outra; largar no espaço vazio move para a raiz |
| desfazer / refazer | `Ctrl+Z` / `Ctrl+Y` (ou `Edit > Undo`), com o nome da ação no menu |

Regras que valem a pena saber:

- criar com **um** objeto selecionado torna o novo objeto **filho** dele; com zero ou vários,
  o objeto nasce na raiz;
- os nomes são únicos (`Cube`, `Cube 2`, …): duas linhas iguais na árvore seriam
  indistinguíveis para quem clica;
- **duplicar copia a subárvore inteira** (um carro sem as rodas não serve) com ids novos,
  deslocada uma unidade em X e Z para não ficar escondida atrás do original; se a seleção
  tiver um pai e um filho, só o pai é duplicado — o filho vem dentro da cópia;
- **ids nunca são reaproveitados**: apagar e criar não devolve o id do objeto apagado;
- **mudar de pai não move o objeto**: os números locais ficam como estão e o objeto passa a
  acompanhar o novo pai, que é o que "ser filho" significa;
- arrastar para dentro de si próprio ou de um descendente é recusado — a linha fica vermelha
  durante o arrasto e a barra de estado diz porquê.

#### Undo e redo (Fase 37)

Tudo o que muda a cena passa por um comando: criar, apagar, duplicar, mudar de pai, os campos
do inspector e os arrastos dos gizmos. O `Edit` mostra o nome da próxima ação ("Undo Move X"),
por isso não é preciso adivinhar o que o `Ctrl+Z` vai fazer.

- **um arrasto = um passo**: mexer uma seta durante dois segundos e largar é *um* `Ctrl+Z`, não
  sessenta. O mesmo para arrastar um `DragValue` no inspector;
- desfazer **depois de gravar** e voltar a fazer a mesma coisa deixa a cena "limpa" outra vez
  (o `*` do título desaparece), porque o editor compara a posição na pilha com a do último
  save, em vez de guardar um "sujou" que nunca volta atrás;
- abrir ou criar uma cena **esvazia a pilha**: desfazer para dentro do ficheiro anterior seria
  uma surpresa;
- a pilha guarda 256 passos; passando disso, o mais antigo cai.

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

### Ferramentas e gizmos (Fases 35a e 35b)

Uma barra flutua no canto superior esquerdo do viewport com as ferramentas. As teclas são as
mesmas de qualquer pacote 3D:

| Tecla | Ferramenta | O que faz |
|---|---|---|
| `Q` | Select | clicar escolhe objetos; nenhum gizmo é desenhado |
| `W` | Move | três setas (X vermelho, Y verde, Z azul) transladam o objeto |
| `E` | Rotate | três anéis rodam o objeto em torno de X, Y ou Z |
| `R` | Scale | três cubos (um por eixo) e um cubo central que escala os três ao mesmo tempo |

O gizmo aparece **só** quando há exatamente um objeto selecionado. Com zero ou com vários não
há handles: um gizmo que significasse "a média destes objetos" seria uma funcionalidade que
ninguém pediu.

Como arrastar:

- **setas do Move**: agarre a seta e arraste. O objeto anda **só** ao longo desse eixo, e a
  distância é a do pointer projetado no eixo — não a do movimento do rato no ecrã, por isso
  arrastar na diagonal não "escorrega" para os outros eixos;
- **quadrado central** do Move: move no plano paralelo ao ecrã, útil para pôr objetos à altura
  dos olhos sem mexer em profundidade;
- **anéis do Rotate**: agarre o anel e arraste; o ângulo é o que o pointer varreu no plano do
  anel. Passar dos ±180° continua a rodar em vez de saltar;
- **clique fora dos handles** volta a selecionar (é o comportamento do `Q`);
- **cubos do Scale**: agarre um cubo e arraste ao longo do eixo. Arrastar uma vez o
  comprimento do handle **duplica** a escala, arrastar meia volta reduz para metade; o fator
  é medido a partir do ponto onde agarrou, não do centro, para que o mesmo gesto dê o mesmo
  resultado em qualquer ponto do handle;
- **cubo central**: escala os três eixos ao mesmo tempo, na direção em que começou a
  arrastar. Se agarrou exatamente no centro (onde a direção é indefinida), arrastar para a
  direita aumenta;
- **os handles de escala seguem os eixos do objeto**, não os do mundo: num objeto rodado 45°,
  arrastar a seta "X" muda `scale.x` e a seta aponta para a aresta que muda;
- **não há espelhamento por arrasto**: o fator nunca chega a zero (mínimo 0.01) nem fica
  negativo. Um `scale` negativo escrito à mão no inspector continua válido e continua
  negativo;
- **filho de um objeto rodado** anda ao longo da seta que vê, porque o deslocamento é aplicado
  no espaço do pai.

Quando uma ferramenta está armada, o objeto selecionado ganha uma **caixa tracejada** em
volta da sua AABB — a mesma caixa sólida que o viewport desenha em todos os objetos deixaria
de dizer qual é o que vai mudar.

Enquanto arrasta, o viewport não seleciona nada: o mesmo clique que agarrou a seta iria também
apanhar o objeto que está atrás dela. Um arrasto conta como **uma** alteração — o `*` do título
aparece ao largar o botão, não a cada frame. (O undo continua a chegar na Fase 37; guardar o
estado inicial do arrasto já está feito, precisamente para lá.)

Arrastar para fora do viewport continua a mover o objeto; largar fora também termina o arrasto.

## 3.3 Inspector (o MVP do editor)

Com a Fase 33 o editor passa a **montar** uma cena, não só a vê-la. Ao abrir ou criar uma
cena, o primeiro objeto fica selecionado e o inspector mostra-o. As secções são
colapsáveis:

| Secção | Campos |
|---|---|
| **Transform** | posição (X/Y/Z, passo 0.1), rotação em **Euler graus** (o quatérnio é convertido), escala |
| **Mesh** | tipo (none/cube/sphere/plane/obj) e os parâmetros do tipo; para `obj`, o caminho |
| **Material** | cor base (RGBA), caminho da textura, `specular`, `shininess` |
| **Light** | direção (o sentido em que a luz viaja), cor, `ambient`, `specular`, `shininess` |
| **Camera** | posição, alvo, `up`, `fov`, `near`, `far` |

Cada alteração aplica-se **imediatamente** ao viewport e marca a cena como alterada (o `*`
no título). As secções de luz e câmara só aparecem nos objetos que são luz ou câmara.

Notas:

- a rotação é editada em graus e convertida para quatérnio ao gravar; os valores são
  normalizados, por isso uma rotação equivalente pode aparecer noutros ângulos depois de
  reabrir;
- `specular`/`shininess` são os campos que o renderer GLES 2.0 consome. `roughness` e
  `emissive` pertencem à Production Pack ainda pendente, e não há slider para um campo que
  não faria nada no ecrã;
- o **undo não existe** até à Fase 37: uma alteração é definitiva, e é por isso que o aviso
  de alterações por gravar já existe desde a Fase 31;
- com vários objetos selecionados o inspector diz "N objects selected" e edita o
  **primário** (o primeiro clicado).

**MVP:** nova cena → selecionar o cubo → mudar posição, cor e escala no inspector → gravar
→ fechar → reabrir. Tudo preservado.

## 3.4 Projetos e assets

Um **projeto** é uma pasta com `project.toml`:

```text
my-project/
├── project.toml      name = "my-project"
│                     scenes = ["scenes/level.scene"]
├── assets/           importados: models/ textures/ sounds/ scenes/
└── scenes/           as cenas do projeto
```

### Criar e abrir

- `File > New Project…` — escolhe-se a **pasta-mãe** e escreve-se o nome; o editor cria
  `my-project/` com `assets/` e `scenes/` e abre-o.
- `File > Open Project…` — lista pastas e mostra `project.toml` dentro delas; confirmar com
  `Open` abre a pasta atual.
- Abrir pela linha de comando: `cargo run -p giochi-editor -- --project /caminho/do/projeto`.

Com um projeto aberto, `Ctrl+O` e `Ctrl+S` começam e gravam em `scenes/`. Gravar uma cena
dentro do projeto acrescenta-a a `scenes = [...]` em `project.toml` — é essa lista que o
jogo lê para encontrar as cenas.

Um projeto que lista uma cena inexistente **abre na mesma**, com aviso na barra de estado.
Recusar esconderia os assets que estão bem.

### Asset browser

| Elemento | O que faz |
|---|---|
| filtros `All` / `Models` / `Textures` / `Sounds` / `Scenes` | filtram a listagem, que é lida de `assets/` |
| `Refresh` | relê a pasta (a listagem é uma fotografia, não um `read_dir` por frame) |
| `Import Asset…` (`Ctrl+I`) | copia um ficheiro para `assets/<tipo>/`, com `nome-1.ext` se o nome já existir |
| clique numa linha | mostra o preview (imagem real para texturas, cubo wireframe para modelos, nº de objetos para cenas) |
| arrastar para o viewport | um `.obj` instancia um nó no chão sob o cursor |

**Só `.obj` instancia.** Um `.gltf`/`.glb` aparece na lista marcado `(no loader)` e, ao ser
arrastado, diz que o loader glTF pertence à Production Pack ainda pendente e que `.obj` é o
que funciona. Esconder o ficheiro pareceria uma importação falhada; não fazer nada seria
pior do que dizer porquê.

Arrastar uma **textura** para o viewport **não** faz nada útil: o destino é o slot
`Material > texture` do inspector, que aceita o drop e escreve o caminho relativo ao
projeto (`assets/textures/...`) no material.

Importar duas vezes o mesmo ficheiro cria `name.png` e `name-1.png`: uma cena que já aponta
para a primeira cópia nunca é sobrescrita.

## 3.4.1 Recentes, autosave e saída (Fase 38)

**Recentes.** `File > Open Recent` lista as últimas dez cenas e projetos, com o caminho
completo na dica. A lista vive em `$XDG_CONFIG_HOME/giochi-editor/settings.toml` (ou
`~/.config/giochi-editor/`), por isso sobrevive a reinícios; `--config DIR` muda a pasta (útil
numa instalação portátil). `File > Clear Recent` limpa a lista no ficheiro também.

**Autosave.** De minuto a minuto (configurável em `autosave_seconds`; `0` desliga), e **só com
alterações por gravar**, o editor escreve `<cena>.scene.autosave` **ao lado** da cena. Nunca
toca no ficheiro da cena: o único `Save` é o seu. Uma cena nunca gravada escreve em
`<config>/autosave/`.

**Recuperação.** `File > Recover autosave` só está ativo quando o autosave é **mais recente**
do que a cena — um autosave mais antigo descreve trabalho que já foi gravado, e oferecê-lo
seria convidar a perder o que é novo. O que é recuperado fica marcado como alterado, porque o
ficheiro em disco não é ele.

**Sair.** Fechar a janela, `Esc` ou `File > Exit` com alterações por gravar abre um diálogo:
`Save and quit`, `Discard` (precisa de dois cliques, porque é a única ação destrutiva do
editor) ou `Cancel`. Numa cena nunca gravada, `Save and quit` abre o diálogo de gravação e sai
quando o ficheiro estiver escrito.

## 3.5 O diálogo de ficheiros

Todos os diálogos são desenhados pelo próprio editor (egui), não o `rfd`: no GIOCHI-OS não
há `xdg-desktop-portal` nem D-Bus de sessão, e um diálogo que só funciona no desktop do
programador é pior do que nenhum. O mesmo diálogo serve para abrir/gravar cenas, abrir
projetos, criar projetos e importar assets — muda o título, o que lista e o que o botão
confirma. Tem `Up`, `Refresh` e campo de nome. ADR-064 regista a troca.

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
| `editor/dialog.rs` | diálogo de abrir/gravar/projeto/import desenhado em egui |
| `project.rs` | `Project`, `AssetKind`, `AssetEntry`, importação com nomes únicos, scan |
| `panels/asset_browser.rs` | filtros, listagem, preview, `AssetDrag`, drop de textura |
| `editor/theme.rs` | paleta e escala tipográfica do editor |

O desenho do viewport acontece **antes** do egui, com o retângulo que o layout produziu no
frame anterior: assim uma linha projetada uns pixels fora do viewport não aparece por cima
de um painel.

---

## 6. Pendências conhecidas (fim da Fase 34)

| Pendência | Fase |
|---|---|
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
| "open a project before importing assets" | importar sem projeto | `File > New Project…` primeiro |
| um `.gltf` não instancia | loader glTF pendente (Production Pack) | usar `.obj` |
