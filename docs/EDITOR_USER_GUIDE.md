# giochi-editor — guia do utilizador

Editor visual de cenas da GIOCHI-ENGINE. Este documento é o guia **da Fase 30**: como
abrir o editor, o que já está na janela e o que cada painel espera das fases seguintes.
Cresce uma secção por fase até ao guia completo da Fase 40.

---

## 1. Abrir

```bash
cargo run -p giochi-editor --release
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
torna a aceitação da fase verificável sem clicar.

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
| Hierarchy | árvore da cena, seleção, destaque no viewport | 32 |
| Inspector | Transform, Mesh, Material, Light, Camera | 33 |
| Asset Browser | árvore de assets do projeto, import, drag-to-viewport | 34 |
| Viewport | render da cena, câmara orbit, grid e eixos | 31 |

O viewport é preto de propósito: pinta a cor de fundo da cena, e é a Fase 31 que lhe põe
um cubo, um grid e uma câmara.

### Menu

`File`, `Edit`, `View`, `Play` e `Help` abrem. As entradas que ainda não existem estão
**desativadas** e, ao passar o rato, dizem em que fase chegam — não desaparecem, para o
mapa de menus não mudar de fase para fase.

Já funcionam:

| Ação | Onde |
|---|---|
| Sair | `File > Exit`, ou `Esc`, ou o botão de fechar da janela |
| Mostrar/esconder Hierarchy, Inspector e Asset Browser | `View` |

Esconder um painel devolve o espaço ao viewport; o layout do egui é elástico, por isso
arrastar as bordas dos painéis também funciona.

---

## 3. Teclado

| Tecla | Efeito |
|---|---|
| `Esc` | Sair — exceto quando a consola está aberta ou um campo de texto tem o foco |
| `F2` | Abrir/fechar a consola in-game do `giochi-engine` |

`Q`/`W`/`E`/`R` (select/translate/rotate/scale) chegam com os gizmos, nas Fases 35a/35b;
`Ctrl+Z`/`Ctrl+Y` chegam na Fase 37. O estado `EditorMode` já existe para os receber.

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
| `src/lib.rs` | opções, ciclo, `run()` |
| `src/main.rs` | linha de comando (`--frames`, `--help`) |
| `editor/editor_state.rs` | cena aberta, seleção, modo, dirty, painéis |
| `editor/layout.rs` | painéis, menu, viewport; devolve `LayoutMetrics` |
| `editor/theme.rs` | paleta e escala tipográfica do editor |

---

## 6. Pendências conhecidas da Fase 30

| Pendência | Fase |
|---|---|
| Abrir/gravar cenas, diálogos de ficheiro (`rfd`) | 31 e 38 |
| Câmara do viewport (orbit/pan/zoom) e independência da câmara da cena | 31 |
| Destaque de seleção e raycast | 32 |
| Undo/redo global | 37 |
| Preferências, atalhos documentados no menu, About | 40 |

`rfd` não entra na Fase 30 porque não há nenhum diálogo para abrir; entra na Fase 31, com
o primeiro `File > Open Scene`.

---

## 7. Problemas comuns

| Sintoma | Causa | Solução |
|---|---|---|
| `failed to create window` com `SDL_VIDEODRIVER=kmsdrm` | não há TTY/DRM na sessão | deixar o SDL escolher, ou `SDL_VIDEODRIVER=x11` |
| aviso `kmsdrm video driver unavailable` | comportamento normal num desktop | ignorar ou fixar `SDL_VIDEODRIVER` |
| janela abre e fecha logo | `--frames` foi passado | correr sem `--frames` |
| texto pequeno em HiDPI | o backend usa `pixels_per_point = 1.0` | pendente (Fase 40, preferências) |
