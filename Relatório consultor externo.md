📄 Relatório 2 — Rede P2P (para a Equipe da Engine / GIOCHI-ENGINE-3D)

Assunto: Requisitos para adicionar suporte a rede P2P à GIOCHI-ENGINE-3D

Status: 🔴 Ausente. A engine não possui módulo de rede. É necessária uma nova camada em Rust, integrada via ABI C.

O contrato da ABI C é explícito: "networking" está listado como fora do escopo da versão atual. Nenhum crate do workspace (giochi-engine, giochi-engine-capi, giochi-engine-3d, giochi-engine-lua) implementa soquetes, descoberta de pares ou sincronização de estado. A engine hoje é estritamente single-player.

Arquitetura proposta para a camada de rede (crate giochi-net):

Componente Especificação
Linguagem Rust puro (sem dependências que impeçam cross-compile para aarch64-unknown-linux-gnu)
Transporte UDP via std::net::UdpSocket (evitar tokio multi-thread; manter o crate leve e síncrono para portabilidade ARM64)
Descoberta de pares mdns-sd para descoberta automática em LAN (essencial para consoles portáteis e partidas rápidas)
Serialização postcard ou bincode — formato binário compacto, baixo overhead
Arquitetura de jogo Host-Autoritativo: um jogador (host) simula inimigos, ondas, torre e loja; os demais (clientes) enviam input e recebem snapshots de estado
Protocolo de mensagens Enum binário com variantes: Handshake, Input, PlayerState, EnemySnapshot, WaveState, TowerState, ShopOpen, ShopBuy, Ping/Pong
Integração com a engine Exposição via ABI C no crate giochi-engine-capi, seguindo o padrão de handles opacos u64 e tipos #[repr(C)] já usado por toda a engine. Funções como net_host_create, net_client_join, net_send_input, net_poll_snapshot
Binding Lua Opcional (fallback), seguindo o padrão do giochi-engine-lua

Requisitos específicos para o alvo ARM64 / GIOCHI-OS:

· O crate deve compilar para aarch64-unknown-linux-gnu sem dependências de sistema além do libc padrão.
· As funções de rede devem ser não-bloqueantes (usar sockets em modo não-bloqueante ou uma thread dedicada por peer) para não travar o loop de timestep fixo da engine (60 Hz).
· O GIOCHI-OS precisará declarar permissão de rede e portas UDP no manifesto do app (o formato de manifesto já existe no assemble.mjs do GIOCHI-OS).

Ação necessária para a Equipe da Engine:

1. Criar o crate giochi-net no workspace, com as dependências mínimas (mdns-sd, postcard/bincode).
2. Implementar o protocolo de mensagens e a máquina de estados Host/Client.
3. Adicionar as funções de rede à ABI C (giochi-engine-capi), com handles opacos e retorno int32_t (0 = sucesso, negativo = erro), seguindo o contrato existente.
4. Escrever testes de integração que rodem dois processos locais (host e client) trocando snapshots via UDP, garantindo que o loop da engine não seja bloqueado.
5. Validar o cross-compile para aarch64-unknown-linux-gnu no CI, assim como já é feito para a engine principal.


Interest Management (obrigatório): o crate giochi-net deve implementar AOI por cliente. O host mantém uma lista de inimigos vivos e, a cada snapshot (10–20 Hz), envia a cada cliente somente os inimigos dentro do raio de interesse calculado a partir da câmera informada pelo cliente. Estado global (torre, onda, boss, nemesis – posição defasada) permanece em broadcast. Funções adicionais na ABI C: net_set_camera_pos e net_poll_snapshot_filtered.


📄 Addendum — Issue para GIOCHI-ENGINE-3D (rede P2P + AOI)

Título: feat(net): adicionar crate giochi-net com Interest Management para co-op PvE

Corpo:

```markdown
## Contexto

A GIOCHI-ENGINE-3D não possui módulo de rede (confirmado no C_ABI_CONTRACT.md:
"networking está fora do escopo"). Precisamos de suporte P2P para um jogo
co-op PvE (tower defense) rodando no GIOCHI-OS em Linux ARM64.

## Arquitetura

- **Host-autoritativo**: um jogador simula inimigos/ondas/torre/loja.
- **Transporte**: UDP via `std::net::UdpSocket` (síncrono, sem `tokio`).
- **Descoberta**: `mdns-sd` para LAN.
- **Serialização**: `postcard` ou `bincode`.
- **Alvo de compilação**: `aarch64-unknown-linux-gnu`.

## Protocolo de mensagens (enum binário)

| Variante | Direção | Campos principais |
|---|---|---|
| `Handshake` | C→H | player_id, version, controller_type |
| `Input` | C→H | tick, player_id, move_dir, aim_dir, action_flags, camera_pos |
| `PlayerState` | H→C | player_id, pos, hp, abilities, score |
| `EnemySnapshot` | H→C (unicast AOI) | tick, enemies: Vec<{id, pos, hp, kind, boss_phase}> |
| `WaveState` | H→C (broadcast) | number, timer, is_boss, boss_id, boss_hp_pct, mode (Campaign/Endless) |
| `TowerState` | H→C (broadcast) | hp, max_hp, armor |
| `NemesisState` | H→C (broadcast, posição defasada ~5s) | id, last_seen_pos, wave_survived, tier |
| `ShopOpen` / `ShopBuy` | H→C / C→H | items, gold / player_id, item_id |
| `Ping` / `Pong` | ambos | timestamp (RTT) |

## Interest Management (OBRIGATÓRIO)

O host envia snapshots de inimigos por **unicast**, filtrados pela
**Area of Interest (AOI)** de cada cliente:

- Cada cliente envia `camera_pos` junto com o input.
- AOI = raio de ~1,5 × meia-diagonal da tela do cliente.
- Apenas inimigos dentro da AOI são enviados àquele cliente.
- Estado global (torre, onda, boss, nemesis defasado) permanece em broadcast
  (dado pequeno, sempre relevante).

**Por quê**: mapa é 4×4 telas (16 telas totais). Sem AOI, broadcast de
~150 inimigos × 4 clientes = ~48 KB/s de upload no host ARM64. Com AOI,
cai para ~4 KB/s por cliente.

## Novas funções na ABI C (giochi-engine-capi)

Seguindo o padrão de handles opacos `u64` e `#[repr(C)]` do contrato existente:

- `net_host_create(port) -> u64`
- `net_client_join(addr) -> u64`
- `net_send_input(handle, input, camera_pos) -> i32`
- `net_set_camera_pos(handle, x, z) -> i32`
- `net_poll_snapshot(handle, out_snapshot) -> i32`
- `net_poll_snapshot_filtered(handle, out_snapshot, max_enemies) -> i32`

Retorno `int32_t`: 0 = sucesso, negativo = erro.

## Requisitos de performance do host (ARM64)

- IA de inimigos simulada a **20 Hz** (desacoplada do render 60 Hz).
- Limite duro de inimigos vivos (ex: 150) com fila de spawn.
- Pathfinding: grid A* com cache de rotas **ou** steering behavior sem grid.
- Rede não-bloqueante: sockets em modo não-bloqueante ou thread dedicada por
  peer. O loop de timestep fixo da engine **nunca** pode ser bloqueado por I/O.

## Testes de aceitação

1. Dois processos locais (host + client) trocam snapshots via UDP sem travar
   o loop da engine.
2. Cliente fora da AOI recebe 0 inimigos; cliente no centro recebe todos
   os da sua região.
3. Cross-compile para `aarch64-unknown-linux-gnu` passa no CI.
4. Nemesis: posição em broadcast defasada ~5s; cliente na AOI recebe posição
   real.

## Fora do escopo desta issue

- Touch (`SDL_Finger`) — extensão separada, prioridade baixa (ver issue do OS).
- NAT traversal / relay para partidas fora da LAN — LAN only por ora.