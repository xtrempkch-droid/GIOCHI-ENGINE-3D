# Animação

O módulo `giochi_engine::animation` reúne animação de spritesheet, easing, tweens e uma
máquina de estados por eventos.

## Spritesheet

Os frames usam retângulos de origem em pixels da textura. `SpriteAnimation::from_strip`
constrói uma faixa horizontal de frames iguais; `SpriteAnimation::new` aceita recortes e
durações independentes, por exemplo regiões de um atlas.

```rust
use giochi_engine::animation::{PlaybackMode, SpriteAnimation};
use giochi_engine::Vec2;

let mut walk = SpriteAnimation::from_strip(
    texture,
    Vec2::new(32.0, 32.0),
    6,
    0.12,
    PlaybackMode::Loop,
)?;
walk.update(time.delta)?;
walk.draw(renderer, destination)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`PlaybackMode::Once` pára no último frame; `Loop` recomeça no primeiro; `PingPong` inverte
a direcção sem repetir os endpoints. `pause`, `play` e `reset` controlam a reprodução.
O desenho usa a câmara e o batch normal de sprites. As animações aceitam até
`MAX_ANIMATION_FRAMES`; deltas inválidos são devolvidos como `AnimationError`.

## Tween e easing

`Tween<T>` interpola tipos que implementam `CurveValue` (`f32`, `Vec2` e `Color` incluídos).
As curvas `Easing::{Linear, EaseIn, EaseOut, EaseInOut}` são aplicadas ao progresso
normalizado; `update` recebe segundos e limita o valor ao endpoint.

```rust
use giochi_engine::animation::{Easing, Tween};
use giochi_engine::Vec2;

let mut position = Tween::new(
    Vec2::ZERO,
    Vec2::new(300.0, 0.0),
    1.0,
    Easing::EaseInOut,
)?;
let position_now = position.update(time.delta)?;
# let _ = position_now;
# Ok::<(), giochi_engine::animation::AnimationError>(())
```

## Máquina de estados

`AnimationStateMachine` liga pares `(estado, evento)` a um novo estado. Os IDs são
definidos pela aplicação; use `current_state` para seleccionar o clip correspondente.
Um evento sem transição é ignorado e `trigger` devolve `false`.

```rust
use giochi_engine::animation::AnimationStateMachine;

let mut machine = AnimationStateMachine::new(0); // idle
machine.add_transition(0, 1, 2)?; // evento "run"
if machine.trigger(1) {
    assert_eq!(machine.current_state(), 2);
}
# Ok::<(), giochi_engine::animation::AnimationError>(())
```

## Exemplo

`cargo run -p giochi-engine --example animation_demo` alterna idle/run com Espaço,
anima o sprite com um tween e aceita `--frames 240 --auto` para uma execução limitada
e determinística.

## Âmbito e custos

O módulo não tem flag de feature: não puxa dependência nenhuma e o linker descarta o que o
jogo não chama (ADR-056). Não há *blending* entre animações nem camada de IA — a máquina de
estados responde "em que estado estou" e o jogo escolhe o clip. *Root motion*, eventos de
frame e *blend trees* não estão implementados.

`Tween<T>` interpola qualquer tipo que implemente `CurveValue`, o trait publicado com as
partículas (`giochi_engine::particles::CurveValue`). É aritmética, não um conceito de
partícula; o caminho público actual mantém-se e o trait pode mudar de módulo com um
re-export.

`animation_render` e `animation_demo` são os dois consumidores de referência: o teste prova
por pixel que a região de origem escolhida chega ao framebuffer, a demo mostra idle/run a
alternar com Espaço.

```bash
cargo test -p giochi-engine --test animation_render
cargo test -p giochi-engine --test perf_regression   # 256 tweens a 0 alocações/frame
```
