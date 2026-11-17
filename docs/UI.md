# UI (Fase 21)

Widgets, layouts e temas para menus e HUDs. Construído sobre o backend egui da Fase 20.

---

## 1. O que existe

| Peça | O que é |
|---|---|
| `ui::Button`, `Label` (via `Ui::label`), `Slider`, `Checkbox`, `TextInput`, `ComboBox`, `ProgressBar`, `Image` | widgets |
| `Ui::vertical`, `Ui::horizontal`, `Ui::group` | layouts |
| `Ui::label`, `heading`, `muted`, `separator`, `space` | texto e espaçamento |
| `Theme` com `dark`, `light`, `retro` | paleta + escala tipográfica |
| `Scene::wants_ui` / `Scene::render_ui` | o gancho da cena |

```bash
cargo run -p giochi-engine --example ui_demo --features devtools
cargo run -p giochi-engine --example ui_demo --features devtools -- --theme retro
```

`ui_demo` tem menu principal, opções (slider de volume, checkbox de fullscreen, input de
nome, combo de tema, barra de progresso) e uma tela "playing". No console: `theme retro`.

---

## 2. O gancho da cena

```rust
impl Scene for Menu {
    fn wants_ui(&self) -> bool { true }

    fn render_ui(&mut self, ui: &mut Ui<'_>) {
        if ui.button(&Button::new("Start")) {
            self.state = State::Playing;
        }
    }
}
```

`wants_ui` existe por um motivo de custo, não de estilo: responder `false` faz o engine
**não rodar egui nenhum** naquele frame. Uma cena sem UI não paga nada pela camada de UI.
Os dois métodos vêm juntos por construção — um `render_ui` implementado com `wants_ui`
devolvendo `false` nunca é chamado, e isso é um bug do jogo, não do engine.

O `Ui` tem um lifetime preso ao frame: não dá para guardá-lo num campo, o que impede o
clássico "guardei o contexto e desenho fora do frame".

---

## 3. Widgets

Os widgets são valores do engine. Nenhum tipo do `egui` aparece na assinatura de um jogo —
é o que permite a ABI C da Fase 24 e o que impede o jogo de quebrar quando o egui mudar
(ADR-055).

```rust
// Botão: retorna true no frame do clique.
if ui.button(&Button::new("Quit").width(200.0)) { quit = true; }
if ui.button(&Button::new("Continue").disabled()) {}   // nunca dispara

// Slider: dono do valor; `true` quando mudou.
let mut volume = Slider::new(0.0, 1.0).with_value(state.volume).with_step(0.05);
if ui.slider(&mut volume) { state.volume = volume.value(); }

// Checkbox
let mut fullscreen = Checkbox::new("fullscreen").with_checked(state.fullscreen);
if ui.checkbox(&mut fullscreen) { state.fullscreen = fullscreen.is_checked(); }

// Texto
ui.text_input(&mut state.name);           // state.name: TextInput
let typed = state.name.text();

// Lista
let mut combo = ComboBox::new("theme").option("dark").option("light").with_selected(0);
if ui.combo_box(&mut combo) { theme = ThemeKind::ALL[combo.selected()]; }

// Barra
ui.progress_bar(&ProgressBar::new(state.volume));

// Imagem (textura da engine, sem upload nem cópia)
ui.image(&Image::new(texture, 64.0, 64.0));
```

### O que cada widget garante

| Widget | Garantia |
|---|---|
| `Button` | largura só é aceita se finita e positiva; `disabled()` desenha apagado e devolve `false` |
| `Slider` | valor **sempre** dentro do intervalo; `NaN` vira o mínimo; intervalo invertido ou não-finito vira `0..=1` |
| `Checkbox` | o estado é do widget; o jogo lê `is_checked()` |
| `TextInput` | `with_max_chars` corta em fronteira de caractere (`café` com limite 3 = `caf`) |
| `ComboBox` | índice fora do intervalo é ignorado; lista vazia devolve `""` |
| `ProgressBar` | fração presa em `0..=1`; `NaN` vira 0 |
| `Image` | tamanho não-positivo vira `1x1` em vez de sumir |

Todas essas regras existem porque os valores vêm de um jogo ou de um arquivo de
configuração, e um widget que panicaria num `NaN` seria um debug tool que derruba o jogo.

---

## 4. Layouts

```rust
ui.vertical(&mut |ui| {
    ui.heading("OPTIONS");
    ui.separator();
    ui.horizontal(&mut |ui| {
        ui.label("volume");
        ui.slider(&mut volume);
    });
    ui.group(&mut |ui| {
        ui.muted("agrupado, com a cor de superfície do tema");
    });
});
```

`vertical`, `horizontal` e `group` recebem `&mut dyn FnMut(&mut Ui<'_>)` — um trait object,
não um genérico, porque uma função pública genérica não passa na regra C-shaped do crate
(`tests/c_shape.rs`). O efeito para o jogo é o mesmo de uma API genérica.

---

## 5. Temas

```rust
engine.set_ui_theme(Theme::retro());
```

| Tema | Superfície | Texto | Acento | Cantos | Fonte |
|---|---|---|---|---|---|
| `dark` | `#121620` | `#dee8f6` | `#5c8fd8` | 4 px | proporcional |
| `light` | `#f2f4f8` | `#1a1f28` | `#2f6fd0` | 4 px | proporcional |
| `retro` | `#000000` | `#44ff66` | `#88ffaa` | **0** | monoespaçada |

O tema é um valor, não um global: o engine o aplica ao contexto egui no início do frame,
então dois jogos no mesmo binário (ou duas cenas) podem usar temas diferentes, e um
`--theme retro` é reproduzível.

`Theme` é `#[repr(C)]` com cores em `0xRRGGBBAA` e `_reserved`, para a Fase 24 poder expor
`giochi_ui_set_theme` sem um segundo layout.

---

## 6. A consola e a UI da cena, juntas

As duas dividem **um** contexto egui e **um** atlas de fonte:

```
um passe egui por frame
├── TopBottomPanel::bottom  ← consola (se aberta)
└── CentralPanel            ← cena (se wants_ui)
```

Consequências, todas desejadas:

* uma tecla consumida pela consola não chega à cena (o filtro é antes do `Input`);
* clicar num botão da consola não clica no botão da cena atrás dele;
* existe **um** atlas de fonte, então a VRAM do painel conta a verdade;
* com a consola fechada e a cena sem UI, nenhum passe egui acontece.

`Ui::wants_pointer()` e `Ui::used_rect()` existem para um jogo que tem mouse no mundo (um
editor, um RTS): perguntar antes de agir evita "cliquei no botão e também selecionei o
tanque atrás dele".

### A API de host (Fase 30)

Um programa que conduz o seu próprio ciclo — o `giochi-editor` — desenha egui com o mesmo
backend da engine, sem duplicar pintura:

```rust
use giochi_engine::ui::{translate_event, EguiBackend};

let mut backend = EguiBackend::new();
for event in &events {
    if let (Some(event), _) = translate_event(event, /* wants_keyboard = */ true) {
        egui_events.push(event);
    }
}
let output = backend.run(screen, dt, &egui_events, &mut |ctx| {
    theme.apply(ctx);
    // painéis do host
});
backend.paint(engine.render_mut(), output);
```

O contrato é o de sempre: `run` antes de `paint`, e `release` enquanto o registo de
texturas ainda existe. O ciclo da engine não mudou — continua a usar **um** `EguiBackend`
privado para a consola e para a UI da cena (ADR-062).

---

## 7. Limitações desta fase

| Limitação | Por quê | Consequência |
|---|---|---|
| **UI sobre 3D precisa de outro backend** | os exemplos 3D conduzem `RenderContext3D` diretamente, sem `Engine`/`Scene`, e desenham com GLES, não com `sdl2::render` | entregue em `giochi-engine-3d::ui::GlesHud` com a feature `hud` — ver §9. O que falta é só a tradução de input |
| sem scroll view | `ScrollArea` não foi exposto | uma lista longa precisa de páginas |
| sem tabelas | idem | — |
| sem ícones/fontes customizadas | o tema só escolhe entre proporcional e monoespaçada | — |
| sem *drag & drop*, *tooltip* | não expostos | — |
| bidirecionalidade de teclado (Tab entre campos) | o egui faz; a consola intercepta `Tab` **antes** | dentro da consola, `Tab` completa comando; na cena, `Tab` muda o foco |

A primeira linha era a pendência da fase. Está resolvida em §9: o crate 3D tem o seu
próprio pintor egui sobre GLES 2.0, e o `mini_fps --hud` desenha fps, posição e estado da
física. O que continua de fora é a tradução de eventos (o HUD é de leitura), registado em
`IDEAS.md`.

---

## 8. Custo

| Situação | Custo por frame |
|---|---|
| `devtools` fora do build | zero |
| compilada, cena com `wants_ui == false` e consola fechada | uma chamada virtual que devolve `false` |
| compilada, UI desenhada | um passe egui (layout + tesselação + um `render_geometry` por mesh) |

O `perf_regression` continua em **0 alocações por frame vazio** com a feature compilada: a
cena de teste não quer UI, e nada é construído antes de a resposta ser `false`.

---

## 9. UI sobre 3D (feature `hud` do crate 3D)

A pendência da Fase 21. O `giochi-engine/src/ui` desenha egui com `sdl2::render`, que o
caminho 3D não usa (ADR-037); `giochi-engine-3d::ui::GlesHud` é o mesmo egui com outro
pintor:

```rust
use giochi_engine_3d::ui::GlesHud;

let mut hud = GlesHud::new(render.glow())?;
// ... depois da cena, antes de `end_frame` ...
hud.draw(render.glow(), (width, height), 1.0 / 60.0, &mut |ctx| {
    egui::Window::new("giochi").show(ctx, |ui| ui.label("fps"));
})?;
hud.destroy(render.glow());
```

O pintor são 20 linhas de GLSL e dois buffers. Duas coisas exigem cuidado e estão
resolvidas dentro dele:

* **Sem índices `u32` em GLES 2.0** sem `OES_element_index_uint`: o `Mesh::split_to_u16` do
  egui parte o que passa de 65 535 vértices.
* **Sem VAO em GLES 2.0**: os ponteiros de atributo são ligados a cada chunk.

O `Hud` **não traduz input** ainda: é um overlay de leitura. Um HUD clicável precisa do
mapeamento de eventos que vive em `giochi-engine/src/ui/egui_integration.rs`, e movê-lo para
um crate partilhado é trabalho para quando houver um consumidor que clique em algo.

`cargo run -p giochi-engine-3d --features hud,physics-3d --example mini_fps -- --hud`
mostra fps, posição do olho e, com `--physics`, o número de corpos e se o jogador está
apoiado. `tests/hud_render.rs` desenha duas frames e exige mais de três cores não-fundo no
canto superior esquerdo: um overlay que não pinta é indistinguível de um que funciona, do
lado de fora.

---

## 10. API de referência

```rust
impl Engine {
    fn ui_theme(&self) -> Theme;
    fn set_ui_theme(&mut self, theme: Theme);
}

pub trait Scene {
    fn wants_ui(&self) -> bool { false }
    fn render_ui(&mut self, ui: &mut Ui<'_>) {}
}

pub struct Ui<'a> {
    pub fn theme(&self) -> Theme;
    pub fn available_size(&self) -> Vec2;
    pub fn label(&mut self, text: &str);
    pub fn heading(&mut self, text: &str);
    pub fn muted(&mut self, text: &str);
    pub fn button(&mut self, button: &Button) -> bool;
    pub fn slider(&mut self, slider: &mut Slider) -> bool;
    pub fn checkbox(&mut self, checkbox: &mut Checkbox) -> bool;
    pub fn text_input(&mut self, input: &mut TextInput) -> bool;
    pub fn combo_box(&mut self, combo: &mut ComboBox) -> bool;
    pub fn progress_bar(&mut self, bar: &ProgressBar);
    pub fn image(&mut self, image: &Image);
    pub fn separator(&mut self);
    pub fn space(&mut self, points: f32);
    pub fn vertical(&mut self, contents: &mut dyn FnMut(&mut Ui<'_>));
    pub fn horizontal(&mut self, contents: &mut dyn FnMut(&mut Ui<'_>));
    pub fn group(&mut self, contents: &mut dyn FnMut(&mut Ui<'_>));
    pub fn wants_pointer(&self) -> bool;
    pub fn used_rect(&self) -> Rect;
}
```

Host de egui (Fase 30; §6 acima):

```rust
pub fn translate_event(event: &sdl2::event::Event, wants_keyboard: bool)
    -> (Option<egui::Event>, bool);

pub struct EguiBackend { /* contexto, texturas, vértices */ }
impl EguiBackend {
    pub fn new() -> Self;
    pub fn context(&self) -> &egui::Context;
    pub fn run(&mut self, screen: (u32, u32), dt: f32, events: &[egui::Event],
               ui: &mut dyn FnMut(&egui::Context)) -> egui::FullOutput;
    pub fn paint(&mut self, render: &mut RenderContext, output: egui::FullOutput);
    pub fn release(&mut self, render: &mut RenderContext);
}
impl Default for EguiBackend { /* = new() */ }

impl RenderContext {
    pub fn set_resizable(&mut self, resizable: bool);
}
```
