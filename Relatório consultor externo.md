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






Relatório de Melhorias — Sistema de Texturas 3D da GIOCHI-ENGINE-3D

1. Resumo

A GIOCHI-ENGINE-3D possui suporte parcial a texturas 3D no nível do pipeline gráfico, mas falta uma API pública de alto nível, documentação e exemplo de uso. O código já contém shaders específicos para texturas (textured.vert e textured.frag), e a estrutura Vertex já inclui coordenadas UV (pub uv: [f32; 2]). Porém, não há um TextureHandle 3D exportado, nem funções documentadas para criação, carregamento, upload e vinculação de texturas no contexto 3D.

Isso torna o uso confuso para quem tenta criar um jogo 3D, especialmente porque a documentação principal (C_ABI_CONTRACT.md) foca na API 2D, que já possui GiochiTexture e funções como giochi_texture_create, enquanto a parte 3D não oferece equivalente claro.

2. Estado atual observado

O que já existe

· Shaders dedicados a texturas:
  · textured.vert
  · textured.frag
· O textured.frag declara uniform sampler2D u_texture; e usa texture2D(u_texture, v_uv).
· O textured.vert recebe o atributo a_uv e o repassa ao fragment shader.
· A estrutura Vertex contém pub uv: [f32; 2].
· O renderer menciona shaders “flat and textured”.

O que falta ou está pouco claro

· Não há uma API pública de alto nível para texturas 3D, como Texture3D, TextureHandle ou equivalente.
· Não há funções documentadas para:
  · criar textura;
  · carregar imagem;
  · fazer upload para a GPU;
  · configurar filtros, wrap modes e mipmaps;
  · vincular textura a uma unidade;
  · passar o sampler para o shader.
· O exemplo cube_demo não utiliza texturas.
· A documentação não explica como usar os shaders textured.vert/textured.frag na prática.
· Não está claro se o usuário deve usar glow/OpenGL diretamente ou se a engine pretende expor uma abstração própria.
· A ABI C documentada parece priorizar a parte 2D; não há contrato equivalente para texturas 3D.

3. Problemas para o usuário da engine

1. Ambiguidade de responsabilidade: o usuário não sabe se deve criar texturas manualmente via OpenGL ou se a engine oferece suporte.
2. Falta de exemplo funcional: não há demonstração de uma malha 3D texturizada.
3. API inconsistente: a parte 2D tem GiochiTexture; a parte 3D não tem equivalente claro.
4. Documentação insuficiente: os shaders existem, mas não há guia de integração.
5. Risco de implementações divergentes: sem API oficial, cada usuário pode implementar texturas de forma diferente, gerando incompatibilidades.

4. Solicitações à equipe mantenedora

Solicito que a equipe avalie e implemente/atualize os seguintes pontos:

4.1. Expor uma API pública de texturas 3D

Criar uma abstração oficial, por exemplo:

```rust
pub struct Texture3D { /* handle interno */ }

impl Texture3D {
    pub fn from_file(path: &str) -> Result<Self, TextureError>;
    pub fn from_bytes(data: &[u8], width: u32, height: u32) -> Result<Self, TextureError>;
    pub fn bind(&self, unit: u32);
    pub fn set_filtering(&mut self, min: Filter, mag: Filter);
    pub fn set_wrap(&mut self, wrap_s: Wrap, wrap_t: Wrap);
    pub fn set_mipmaps(&mut self, enabled: bool);
}
```

Ou, se a intenção for manter a API próxima do OpenGL, documentar oficialmente esse caminho com exemplos.

4.2. Integrar texturas ao sistema de materiais/renderização 3D

· Permitir associar uma textura a um material.
· Definir como o sampler é passado ao shader textured.
· Documentar a ordem esperada de bindings e uniforms.
· Garantir compatibilidade com o renderer existente.

4.3. Documentar o fluxo completo

Adicionar uma seção na documentação explicando:

1. Como carregar uma imagem.
2. Como criar a textura na GPU.
3. Como configurar filtros e wrap modes.
4. Como vincular a textura ao shader.
5. Como definir UVs nos vértices.
6. Como renderizar uma malha texturizada.

4.4. Criar exemplo funcional

Adicionar um exemplo, por exemplo textured_cube_demo, contendo:

· Um cubo com UVs;
· Uma textura de teste;
· Uso dos shaders textured.vert e textured.frag;
· Código comentado explicando cada etapa.

4.5. Melhorar tratamento de erros

Definir erros claros para:

· arquivo de imagem inválido;
· formato não suportado;
· falha no upload para GPU;
· textura não vinculada;
· sampler não encontrado no shader.

4.6. Adicionar testes

Sugestões de testes:

· Criação de textura a partir de bytes;
· Upload e bind de textura;
· Renderização de um quad texturizado;
· Verificação de que o shader recebe o sampler correto;
· Teste de fallback quando a textura não é encontrada.

5. Critérios de aceitação

Considerar a melhoria concluída quando:

· Existir uma API pública documentada para texturas 3D;
· Houver pelo menos um exemplo funcional de malha 3D texturizada;
· A documentação explicar o fluxo completo de uso;
· O cube_demo ou um novo demo demonstrar texturas;
· A ABI C, se aplicável, tiver contrato equivalente para texturas 3D;
· Houver testes cobrindo criação, bind e renderização com textura;
· Erros comuns forem tratados e documentados.

6. Prioridade sugerida

Média/Alta, pois texturas são essenciais para jogos 3D e a ausência de uma API clara gera confusão e retrabalho para usuários da engine.

7. Observação final

O suporte parcial já existente nos shaders e na estrutura Vertex indica que a base técnica está presente. O principal gap é de API pública, documentação e exemplo de uso. Uma implementação guiada por esses pontos deve resolver a confusão relatada sem exigir grandes mudanças na arquitetura atual.

---
