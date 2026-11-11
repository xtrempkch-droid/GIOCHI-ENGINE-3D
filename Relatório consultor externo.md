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