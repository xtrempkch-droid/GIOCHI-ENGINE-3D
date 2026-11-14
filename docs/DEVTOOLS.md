# Debug tools (Fase 20)

Consola in-game, desenho de debug e overlay expandido. Tudo atrás da feature `devtools`, que
traz o `egui` como única dependência nova.

---

## 1. As três ferramentas

| Ferramenta | Tecla | O que faz |
|---|---|---|
| console | `F2` | linha de comando, histórico, autocomplete, 10 comandos embutidos |
| debug draw | `F3` | linhas, retângulos, grids e primitivas 3D projetadas, por frame |
| overlay expandido | `F1` | o painel da Fase 15 + VRAM, entidades, comandos, time scale, hot reload |

```bash
cargo run -p giochi-engine --example devtools_demo --features devtools
cargo run -p giochi-engine --example devtools_demo --features devtools -- --frames 600
```

O `devtools_demo` são cubos caindo: `spawn cube 5` no console adiciona cinco, `F3` mostra
AABB e vetor de velocidade de cada um, `F1` mostra o resto.

---

## 2. Ligar

```toml
giochi-engine = { features = ["devtools"] }
```

O `Engine` ganha o campo e a API:

```rust
engine.devtools_mut().commands_mut().register("heal", "restores health", Box::new(|args| {
    format!("healed by {args}")
}));
let flags = engine.devtools().flags();      // god / noclip, o jogo lê
```

Sem a feature, `devtools` não existe, o `Engine` não tem o campo e a consola não compila —
o binário release sai igual ao da Fase 19.

---

## 3. Os comandos embutidos

| Comando | Efeito |
|---|---|
| `help` | lista todos, com a ajuda de cada um, alinhada |
| `clear` | esvazia o scrollback (o histórico sobrevive) |
| `fps` | fps, tempo de frame, p99, draw calls e sprites |
| `entities` | o número que o jogo reportou com `set_entity_count` |
| `time_scale [f]` | sem argumento mostra; com argumento define (0 pausa, teto 8) |
| `god` | alterna `DevFlags::god` — **o jogo** decide o que significa |
| `noclip` | alterna `DevFlags::noclip` |
| `spawn <kind> [count]` | enfileira um `SpawnRequest` para o jogo drenar |
| `reload_shaders` | pede uma recarga; no 2D responde que não há shaders |
| `dump_frame_stats` | fps, p99, max e o tempo de cada sistema |

### Registrando um comando

`register` recebe nome, ajuda e um `FnMut(&str) -> String`. O handler retorna o que imprimir
(string vazia não imprime nada) e fecha sobre o estado do jogo — num engine single-threaded
isso é `Rc<RefCell<_>>`:

```rust
let ammo = Rc::new(RefCell::new(30));
let refill = Rc::clone(&ammo);
engine.devtools_mut().commands_mut().register(
    "ammo",
    "shows or sets the ammo count",
    Box::new(move |args| {
        let mut ammo = refill.borrow_mut();
        if let Ok(value) = args.trim().parse::<i32>() {
            *ammo = value;
        }
        format!("ammo: {}", *ammo)
    }),
);
```

Registrar o mesmo nome **substitui**: é assim que um jogo troca o `spawn` embutido pelo
próprio.

### Por que `god`, `noclip` e `spawn` não fazem nada sozinhos

Um engine não sabe o que é "modo deus" nem o que é um cubo. A consola **oferece o
interruptor e a fila**; o jogo lê e decide (ADR-053):

```rust
while let Some(request) = engine.devtools_mut().take_spawn() {
    world.spawn(request.kind(), request.count());
}
let flags = engine.devtools().flags();
if !flags.god {
    player.take_damage(damage);
}
```

Um jogo que ignora os dois vê um interruptor que não faz nada — o que é honesto.

---

## 4. A consola

| Tecla | Efeito |
|---|---|
| `Enter` | executa a linha |
| `Tab` | completa; um candidato preenche, vários mostram a lista |
| `Up` / `Down` | histórico de comandos enviados (100 linhas) |
| `Esc` | fecha |
| texto, setas, `Home`/`End`, `Backspace`, `Delete` | vão para o editor do egui |

O *scrollback* guarda 512 linhas, com a mais antiga caindo. Linhas de entrada são ecoadas
com `> `, erros em vermelho, avisos em verde.

### O teclado não vaza para o jogo

Este é o detalhe que faz a diferença entre uma consola utilizável e uma inutilizável. O
`Engine` oferece cada evento primeiro ao `DevTools::handle_event`, que devolve `true` quando
o consumiu, e só os não consumidos chegam ao `Input`:

```
evento SDL → DevTools::handle_event → (consumido?) → não: Input → cena
                                           └─ sim: consola
```

Escrever `walk` na consola manda três letras para o editor de texto; um `W` que chegasse ao
`Input` faria o jogador andar enquanto você digita. `tests/devtools_render.rs` e os testes de
unidade fixam o comportamento.

---

## 5. Debug draw

```rust
let debug = engine.devtools_mut().debug_draw_mut();
debug.draw_line_2d(from, to, Color::rgb(0, 255, 0));
debug.draw_rect_2d(rect, Color::rgb(255, 0, 0));
debug.draw_grid_2d(area, 32.0, Color::rgb(40, 40, 60));
debug.draw_aabb_3d(min, max, &view_projection, color);
debug.draw_ray_3d(origin, direction, length, &view_projection, color);
debug.draw_grid_3d(y, extent, divisions, &view_projection, color);
```

As três chamadas 3D levam uma **matriz view-projection column-major** (`[f32; 16]`, o mesmo
layout de `Mat4::to_cols_array` no crate 3D) e projetam para 2D. É por isso que funcionam
sobre o renderer 2D e sobre um overlay de cena 3D sem um passe `GL_LINES` (ADR-054).

Duas regras que o código aplica e a documentação repete:

* **Ponto atrás da câmera é descartado**, não dividido: `w <= 0` daria uma caixa com um
  vértice no infinito, que desenha uma mentira convincente.
* **O buffer é limpo depois de desenhar**, em `DebugDraw::render`. Um buffer que nunca é
  limpo fica cada vez mais lento — exatamente o que a ferramenta existe para detectar.

`F3` liga e desliga a visibilidade. Com ela desligada o buffer é descartado a cada frame,
então não há acúmulo silencioso.

### Quem adiciona geometria

O jogo, não o engine — o engine não sabe onde estão os cubos. No `devtools_demo` isso
acontece no loop, antes de `engine.step`:

```rust
if engine.devtools().debug_draw().is_visible() {
    let debug = engine.devtools_mut().debug_draw_mut();
    for cube in &world.cubes {
        debug.draw_aabb_3d(cube.min(), cube.max(), &matrix, GREEN);
    }
}
engine.step(dt)?;   // desenha e limpa
```

---

## 6. Overlay expandido

`F1` continua a mostrar o painel da Fase 15. Com `devtools`, nove linhas extras entram
embaixo:

```
DRAW 100 SPR 60 MSH 30 UI 10
TEX 7 VRAM 2.0 MB
ENTITIES 42
CMDS 12 CONSOLE ON DRAW3D OFF
TIME_SCALE 1.00
HOT_RELOAD ON RELOADS 3
GOD ON
P99 16.71 MS
```

* `DRAW` separa o total por categoria. `UI` é o que sobra da subtração — não um contador
  próprio, para o painel não mentir quando uma categoria nova aparecer.
* `VRAM` é `Σ largura × altura × 4` das texturas vivas (`TextureRegistry::estimated_vram_bytes`).
  É uma estimativa: um driver pode arredondar o pitch.
* `ENTITIES`, `CMDS`, `CONSOLE`, `DRAW3D` vêm do estado do devtools.

O painel é rasterizado no atlas de bitmap da Fase 15, não em egui: ele precisa funcionar com
a consola fechada, e a 1 Hz o custo é o mesmo de antes.

---

## 7. A integração com o egui, e o que ela não faz

Não usamos `egui-sdl2`. O ADR-052 tem a comparação medida; em resumo: nenhuma versão dele
suporta `egui` 0.28, as recentes exigem Rust 1.92 (acima do piso de 1.86 do engine) e o
backend OpenGL dele tra traria um segundo `glow`. O backend é nosso, em
`giochi-engine/src/ui/egui_integration.rs`:

* os meshes tesselados do egui viram `SDL_RenderGeometry`, a mesma chamada que o batching 2D
  usa;
* as texturas do egui são `TextureHandle`s do registry, então entram na conta de VRAM;
* os eventos SDL são traduzidos um a um, com o flag de "consumido" descrito acima;
* egui só roda **enquanto a consola está aberta**. Fechada, o custo é uma comparação de
  `Option` e duas de tecla.

O que **não** está implementado, e é registrado em vez de silenciado:

| Recurso | Situação |
|---|---|
| cópia para o clipboard (Ctrl+C na consola) | o pedido do egui é logado em `debug`; o SDL clipboard exigiria o `Platform` dentro do backend |
| colar (Ctrl+V) | idem |
| ícones de cursor | não traduzidos; o cursor do sistema continua |
| IME / composição | fora do escopo (teclado latino funciona) |
| `PaintCallback` do egui | ignorado com `debug!` — nada nesta engine o usa |

---

## 8. Custo

| Situação | Custo por frame |
|---|---|
| feature fora do build | zero: módulo e campo não existem |
| compilada, consola fechada | 2 comparações de tecla + `clear` de dois `Vec` vazios + 2 leituras de contador |
| compilada, consola aberta | uma passagem de egui (layout + tesselação + um `render_geometry` por mesh) |

`tests/devtools_render.rs` prova o caso fechado do jeito mais direto possível: a área do
painel fica **exatamente** na cor de limpeza (0 pixels diferentes) e passa a ter mais de 200
pixels diferentes quando a consola abre.

---

## 9. API de referência

```rust
pub mod devtools {
    pub struct DevTools;
    pub struct Console;            // open/close/toggle, push_line, submit, history
    pub struct CommandRegistry;    // register, complete, help_lines, builtin_for
    pub struct DebugDraw;          // draw_*_2d, draw_*_3d, render, clear
    pub struct OverlayExtra;       // os números do painel
    pub enum   Builtin;            // Help, Clear, Fps, Entities, TimeScale, God, …
    pub struct DevFlags { god, noclip }
    pub struct SpawnRequest { kind, count }
    pub const MAX_TIME_SCALE: f32 = 8.0;
}

impl Engine {
    fn devtools(&self) -> &DevTools;
    fn devtools_mut(&mut self) -> &mut DevTools;
    fn time_scale(&self) -> f32;              // existe sem a feature também
    fn set_time_scale(&mut self, scale: f32);
}

impl DevTools {
    fn handle_event(&mut self, event: &Event) -> bool;  // true = consumido
    fn execute(&mut self, line: &str) -> Vec<String>;
    fn draw(&mut self, render: &mut RenderContext);
    fn release(&mut self, render: &mut RenderContext);
    fn take_spawn(&mut self) -> Option<SpawnRequest>;
    fn take_shader_reload_request(&mut self) -> bool;
    fn set_entity_count(&mut self, count: u32);
    fn overlay_lines(&self) -> Vec<String>;
}
```
