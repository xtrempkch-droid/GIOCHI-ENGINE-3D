# Física

Física real via [Rapier](https://rapier.rs), com duas features independentes:

| Feature | Crate | Traz |
| --- | --- | --- |
| `physics-2d` | `giochi-engine` | `PhysicsWorld2D` (Rapier 2D) |
| `physics-3d` | `giochi-engine-3d` | `PhysicsWorld3D` + controlador de personagem (Rapier 3D) |

As duas são independentes e desligadas por omissão: um jogo 2D não liga o Rapier 3D e
vice-versa (ADR-057). Nenhuma assinatura pública menciona um tipo do Rapier.

## Um mundo, um passo fixo

```rust
use giochi_engine::physics::{BodyDesc, ColliderShape, PhysicsWorld2D};
use giochi_engine::Vec2;

let mut world = PhysicsWorld2D::new(Vec2::new(0.0, -30.0))?;
let chao = world.create_body(BodyDesc::static_(Vec2::new(0.0, -0.5)))?;
world.attach_collider(chao, ColliderShape::boxed(20.0, 0.5), 0.9, 0.0)?;
let caixa = world.create_body(BodyDesc::dynamic(Vec2::new(0.0, 4.0)))?;
world.attach_collider(caixa, ColliderShape::boxed(0.5, 0.5), 0.7, 0.0)?;

world.step(time.delta)?;                      // acumula e integra a 60 Hz
world.sync_to_transforms(&mut |handle, transform| {
    // transform.position / transform.rotation seguem o sprite
});
# Ok::<(), giochi_engine::physics::PhysicsError>(())
```

`step` recebe o delta do frame e **acumula**: a 30 Hz roda dois passos de integração, a
60 Hz um, e depois de um travamento roda no máximo `MAX_SUBSTEPS` (4) e descarta o resto.
Um mundo não simula mais tempo do que aconteceu, e não paga a dívida depois — o que
significa que um `dt` de 2 s não teleporta a cena 2 s para a frente.

## Handles

`BodyHandle` é `u64`, começa em 1 e **nunca é reutilizado**. Um handle guardado depois de
`destroy_body` devolve `PhysicsError::InvalidBody` em vez de endereçar outro corpo — o bug
clássico de "o projétil acertou o inimigo que já morreu" passa a ser um erro explícito.
O custo é que os slots crescem; um jogo que cria e destrói corpos para sempre deve reciclar
entidades em vez de emitir handles novos.

## Formatos

- `ColliderShape::boxed(half_width, half_height)`, `circle(radius)`,
  `capsule(half_height, radius)` — **meias medidas**, como o Rapier as toma. Extensões
  zero ou negativas são recusadas (`InvalidShape`), não corrigidas: um collider de área
  zero nunca colide e a causa aparece muito depois.
- `friction` é limitada a `[0, 10]` e `restitution` a `[0, 2]` em vez de recusadas: vêm de
  ficheiros de nível e um erro de digitação deve dar um chão escorregadio, não um crash.
- `BodyDesc::{dynamic, static_, kinematic}`, mais `with_velocity`, `with_rotation`,
  `lock_rotations` (um jogador) e `with_continuous_collision` (uma bala).

## Contactos e raios

`contacts()` devolve os pares do último `step`, como `Started`/`Stopped`, uma vez por
mudança de estado e não por frame: uma caixa em repouso não gera um evento por frame.

```rust
for contacto in world.contacts() {
    if contacto.kind == ContactKind::Started {
        // som, dano, partículas
    }
}
```

`raycast(origem, direção, distância)` normaliza a direção, ignora sensores e devolve
`RayHit { body, point, normal, distance }`. As estruturas de consulta só são
actualizadas por um `step`; por isso o `raycast` faz ele próprio um passo de `dt = 0`
quando o mundo mudou desde o último `step` — criar corpo, anexar collider e lançar o raio
na mesma frame funciona. Um `set_position` seguido de `raycast` sem `step` no meio vê a
posição nova; um `raycast` a meio de um `step` não existe, porque é a mesma thread.

## 3D: controlador de personagem

`PhysicsWorld3D::create_character(posição, raio, meia_altura)` cria um corpo cinemático com
uma cápsula e liga o `KinematicCharacterController` do Rapier. `move_character(handle,
deslocamento, dt)` devolve o quanto o personagem andou na realidade e se ficou apoiado:

```rust
let movimento = world.move_character(jogador, Vec3::new(velocidade, -gravidade * dt, 0.0), dt)?;
if movimento.grounded && input.key_pressed(KEY_SPACE) {
    // saltar
}
```

O controlador desliza em paredes, sobe degraus até 35 cm e cola ao chão a 30 cm. **A
gravidade é responsabilidade do jogo**: um corpo cinemático não é integrado, e um
personagem que nunca recebe deslocamento para baixo nunca toca no chão nem reporta
`grounded`.

## C ABI

`GIOCHI_ABI_VERSION 5` introduziu a física 2D na ABI C e a versão corrente é **6**
(`docs/C_ABI.md` §20): criar/destruir mundo,
criar corpo, anexar caixa, `step`, posição, impulso, destruir corpo, contagem. O mundo é um
ponteiro opaco; os corpos são `uint64_t`. A física 2D está sempre compilada no
`libgiochi.so` porque o header é um contrato e não pode mudar com uma feature.

## Exemplos e testes

```bash
cargo run -p giochi-engine --features physics-2d --example physics_2d_demo
cargo run -p giochi-engine-3d --features physics-3d --example physics_3d_demo
cargo test -p giochi-engine --features physics-2d --test physics_2d
cargo test -p giochi-engine-3d --features physics-3d physics
```

O `physics_2d_demo` imprime, uma vez por segundo, a maior velocidade de qualquer caixa:
uma torre assente reporta `0.000`, um solucionador instável reporta unidades. O
`physics_3d_demo` imprime no fim se o jogador foi bloqueado pela parede, se ficou apoiado
no chão e quantas caixas ainda se movem.
