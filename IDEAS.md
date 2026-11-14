# IDEAS (fora do Growth Pack)

Ideias que apareceram durante as fases 19–24 e que **não** pertencem ao escopo. Registradas
para não serem perdidas nem implementadas por acidente (R6).

## Já pedidas por fases futuras

- [ ] Skeletal 3D (Fase 27+)
- [ ] Editor visual de cena (Fase 30+)
- [ ] Tilemap system (Fase 25+)
- [ ] Post-processing (Fase 26+)
- [ ] Networking / multiplayer (Fase 40+) — já existe `giochi-net`, falta integração de jogo
- [ ] Pathfinding (A* + navmesh)
- [ ] Audio mixer avançado (bus, efeitos)
- [ ] Save/load padronizado
- [ ] Asset streaming / async loading
- [ ] Suporte a Windows/Mac (fora do escopo atual ARM64 Linux)

## Saíram das fases 19–24 e ficaram de fora

- [ ] **Backend egui em GLES para o crate 3D** (HUD sobre cena 3D). O plano da Fase 21 pedia
      "UI sobre 3D (HUD em FPS)"; os exemplos 3D conduzem `RenderContext3D` diretamente, sem
      `Engine`/`Scene`, então o HUD exige um segundo backend egui — ~250 linhas de shader +
      VBO + scissor na mesma `glow` 0.18. É o próximo passo natural e o único item de
      aceitação da Fase 21 não entregue.
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
