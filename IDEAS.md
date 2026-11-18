# IDEAS (fora do Growth Pack)

Ideias que apareceram durante as fases 19–24 e que **não** pertencem ao escopo. Registradas
para não serem perdidas nem implementadas por acidente (R6).

## Já pedidas por fases futuras

- [ ] Skeletal 3D (Fase 27+)
- [x] ~~**Editor visual de cena**~~ — entregue (`giochi-editor`, fases 30–40)
- [ ] Tilemap system (Fase 25+)
- [ ] Post-processing (Fase 26+)
- [ ] Networking / multiplayer (Fase 40+) — já existe `giochi-net`, falta integração de jogo
- [x] ~~**Pathfinding**~~ — a grelha A* chegou em `giochi_engine::nav` (ADR-075); o
      **navmesh** continua em aberto, e é o que a Fase 41+ deve pedir se a pista não for grelha
- [x] ~~**Audio mixer avançado (bus)**~~ — `Bus` Master/Sfx/Music/Ui em `giochi_engine::audio`
      (ADR-078); falta o que sobra: **efeitos** (filtros, reverb, ducking)
- [x] ~~**Save/load padronizado**~~ — `giochi_engine::save::SaveSlots` (ADR-077)
- [ ] Asset streaming / async loading
- [ ] Suporte a Windows/Mac (fora do escopo atual ARM64 Linux)

## Apareceram durante o Salto B (fases 30–40)

Registadas para não serem implementadas por acidente nem esquecidas. Nenhuma delas é
necessária para o que o plano pede nas fases 30–40.

- [ ] **Malha de seleção com vários objetos**: mover/rodar/escalar N objetos em conjunto. O
  gizmo só aparece com exatamente um selecionado; fazê-lo com vários exige decidir o que a
  rotação significa em torno de que ponto.
- [ ] **Renomear inline na hierarquia** (`F2` na linha) — hoje o nome vem do tipo e só muda
  editando o JSON.
- [ ] **Duplicar com arrasto** (arrastar a linha com `Alt` cria a cópia onde cair).
- [ ] **Modo local/global** para os gizmos de translação e rotação (`X` alterna em todos os
  pacotes); a escala já é local por necessidade (ADR-067).
- [ ] **Snap** (grelha e ângulos) com uma tecla modificadora.
- [ ] **Play com física e gameplay dentro do editor**: hoje o preview desenha a cena como está
  (ADR-071). Um runtime de jogo no editor é âmbito da Fase 41+.
- [ ] **Pause/Stop no preview**: só fazem sentido quando houver simulação para pausar.
- [ ] **Preview numa janela do próprio editor (FBO)**: recusado em ADR-064/071; revisitável se
  os drivers Mali melhorarem.
- [ ] **Histórico de undo persistido entre sessões** — hoje a pilha vive na sessão.
- [ ] **Autosave por cena em vez de por sessão**: hoje cada cena tem o seu `.autosave`, mas o
  intervalo é global.
- [ ] **Verificador de pacote a correr o editor numa sessão X virtual** (xvfb) para provar que
  a janela abre, e não só que o modo headless funciona.
- [ ] **Múltiplos viewports** (perspetiva + ortográficas), como nos pacotes grandes.

## Saíram das fases 19–24 e ficaram de fora

- [x] ~~**Backend egui em GLES para o crate 3D**~~ — entregue em
      `giochi-engine-3d::ui::GlesHud` (feature `hud`, ADR-059). O que sobra dele é a
      tradução de input, na secção "3D HUD — what is still missing" abaixo.
- [ ] `GL_LINES` no `giochi-engine-3d` para debug draw denso. O `DebugDraw` da Fase 20
      projeta para 2D, o que custa um `SDL_RenderLine` por segmento — certo para centenas,
      errado para milhares (ADR-054).
- [ ] Clipboard na consola (Ctrl+C/Ctrl+V). O backend recebe o pedido do egui e o registra em
      `debug`, mas ligá-lo ao SDL exige o `Platform` dentro do backend.
- [ ] Ícones de cursor e IME no backend egui.
- [ ] `ScrollArea`, tabelas, tooltips, drag & drop na fachada de widgets (`docs/UI.md` §7).
- [ ] Recarregar `.rs` em runtime (`subsecond`, dylib hot-swap). Fora do escopo do hot reload
      da Fase 19, que trata assets.
- [ ] Recompilar shaders sob demanda a partir da consola no crate 3D (o `reload_shaders`
      embutido só registra o pedido; no 2D responde que não há shaders).
- [ ] Preservar globais Lua entre recargas de script (hoje a VM reinicia; ADR-050).
- [ ] `giochi_ui_*` na ABI v4: a fachada já é C-shaped, falta a exposição (Fase 24c).

## C ABI — what is still not exposed

Everything the Growth Pack asked for is now in the header (ABI v6). What is left is
deliberately out of scope:

- **Physics 3D through C.** Would need a second set of `giochi_physics3d_*` symbols and
  `GiochiVec3`-shaped types at every call. A C game that wants 3D physics is better served
  by linking `rapier3d` (or the Rust crate) than by growing the flat ABI.
- **Particle emitters other than a point.** `AreaEmitter`, `CircleEmitter` and `LineEmitter`
  exist in Rust; each needs its own `#[repr(C)]` description and its own create function.
  Additive whenever someone needs it.
- **Console commands registered from C.** `CommandRegistry::register` takes a Rust closure;
  a C callback would need a function-pointer type, a user-data void*, and a rule for what
  happens when the library outlives the callback. Worth doing with a real consumer.

## 3D HUD — what is still missing

- **Input for the 3D HUD.** `giochi-engine/src/ui/egui_integration.rs` translates SDL events
  into `egui::Event`, but it is `pub(crate)` and keyed on the 2D engine's event type. A
  clickable 3D HUD needs that table lifted into a crate both renderers can use — or a
  `giochi-engine-3d` feature that depends on `giochi-engine`'s input module, which is
  already a dependency, so the work is moving the code and not adding a dependency.
- **A clipboard on KMSDRM.** egui asks for copy/paste; the platform has no clipboard, and
  the request is logged rather than served.

## Apareceram durante o Salto C (fases 41–46)

A lista que o plano do jogo manda manter separada. Nenhuma destas ideias é necessária para
as fases 41–46; todas são o que um jogo de corrida pede *depois* de ser um jogo de corrida.

- [ ] **Drift com pontuação** — o travão de mão já desliza (Fase 41), mas não há sistema de
      pontos nem combo.
- [ ] **Tuning do carro** (suspensão, motor, pneus) — hoje `CarConfig` é um `struct` no
      código; um menu de tuning é Fase 44+ no mínimo.
- [ ] **Carreira com progressão** — troféus, desbloqueios, campeonatos.
- [ ] **Multiplayer split-screen** — precisa de duas vistas no mesmo frame, que o
      `RenderContext3D` não tem (um viewport por frame).
- [ ] **Multiplayer online** — `giochi-net` existe; um jogo de corrida autoritativo é outro
      projecto.
- [ ] **Dano visual e mecânico**.
- [ ] **Replay e ghost da melhor volta** — o `CarState` por frame é determinístico o
      suficiente para um replay, mas não há gravação.
- [ ] **Clima dinâmico** (chuva, noite) — o renderer não tem sombras nem pós-processamento.
- [ ] **Mais pistas e mais carros** — as Fases 42–43 entregam três e um.
- [ ] **VR**.
- [ ] **Time trial como modo explícito** — hoje é implícito: uma corrida sem IA é uma volta
      cronometrada.
- [ ] **Navmesh em vez de grelha** para a IA, se as pistas deixarem de ser corredores.
- [ ] **Física de carro a sério** (raios de suspensão, curvas de pneu): foi deliberadamente
      recusada em `giochi-racer/src/vehicle/car.rs` — o modelo arcade é o produto.
