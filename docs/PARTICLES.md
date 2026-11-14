# Partículas

O módulo `giochi_engine::particles` fornece curvas, emissores 2D e um sistema CPU
limitado. `giochi_engine_3d::particles::BillboardParticleSystem` fornece billboards
3D. Os sistemas são donos da cena: chame `update` durante `Scene::update` e `render`
durante `Scene::render`. Não há atualização global nem custo por frame quando nenhum
sistema é criado.

## Partículas 2D

```rust
use giochi_engine::particles::{presets, ParticleSystem, PointEmitter};
use giochi_engine::Vec2;

let mut explosion = ParticleSystem::new(texture, presets::explosion())?;
let emitter = PointEmitter::new(Vec2::new(200.0, 120.0))?;
explosion.emit(&emitter, 500)?;
explosion.update(1.0 / 60.0)?;
explosion.render(renderer)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

O pool padrão reserva 5.000 partículas; `with_capacity` permite escolher outro limite
até `MAX_PARTICLE_CAPACITY`. Emissões acima do espaço livre são limitadas. Remover
partículas expiradas e emitir novas não aumenta a capacidade. Cada sistema usa uma
textura, e o renderer conserva a ordem de emissão enquanto agrupa sprites consecutivos
da mesma textura. A cor individual é aplicada como cor de vértice, sem quebrar o batch.

Os emissores incluídos são `PointEmitter`, `LineEmitter`, `CircleEmitter` e
`AreaEmitter`. `ParticleConfig` define velocidade, variação, gravidade, arrasto, vida,
tamanho e cores inicial/final. `presets::{explosion, smoke, trail, spark, blood}`
devolvem configurações prontas. `Curve<T>` oferece keyframes lineares e `ease-in`;
`ParticleSystem::set_progress_curve` usa-a para controlar a interpolação ao longo da
vida.

## Billboards 3D

Crie um quad centrado no plano XY, com normal local +Z, e use
`BuiltinShader::Instanced`. O sistema orienta cada quad para a câmara, ordena os
centros de trás para a frente e usa `draw_instanced`. Em drivers sem instancing, o
renderer mantém um fallback correto, mas emite uma chamada por partícula. Para alfa,
ative `GlState::blend` antes de `begin_frame` e desenhe as partículas depois da cena
opaca. A cor é uniforme por sistema neste primeiro backend.

## Exemplos

- `cargo run -p giochi-engine --example particles_2d_demo`: clique e mantenha o botão
  esquerdo para emitir explosão e rasto.
- `cargo run -p giochi-engine --example particles_2d_demo -- --frames 240 --auto`:
  execução determinística limitada.
- `cargo run -p giochi-engine-3d --example particles_3d_demo`: clique para emitir
  billboards 3D.
- `cargo run -p giochi-engine-3d --example particles_3d_demo -- --frames 240 --auto`:
  execução determinística limitada.

## Limites e medição

O teste `particles_render` comprova o limite mensurável de uma chamada para 1.000
partículas 2D com a mesma textura, abaixo do limite padrão de 1.024 sprites por lote.
Esse teste também compara pixels do caminho com tint. O número de 60 FPS depende do
driver e do hardware e não é inferido a partir do teste de batching. A medição de
desempenho do projeto continua em `PERF_LOG.md`.
