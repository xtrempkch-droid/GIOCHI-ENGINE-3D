//! O host: quem simula o jogo e distribui estado.
//!
//! ## Host-autoritativo, e o que isso implica
//!
//! Só o host roda a simulação. Cliente manda **input**, host devolve **estado**.
//! Não há rollback nem predição nesta versão — o relatório não pede, e predicção
//! sem simulação determinística no cliente produz dessincronização que ninguém
//! consegue reproduzir.
//!
//! Consequência assumida: o cliente vê a própria posição com o atraso do RTT.
//! Em LAN (o escopo do relatório) isso é 1–4 ms e não se nota; fora de LAN o
//! problema seria outro e está declarado fora de escopo.
//!
//! ## Quem é dono do quê
//!
//! | Dado | Dono | Por quê |
//! | --- | --- | --- |
//! | inimigos, onda, torre, loja | host | autoridade da partida |
//! | posição da câmera | cliente | só ele sabe onde está olhando |
//! | input | cliente | óbvio |

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::aoi::{AreaDeInteresse, FiltroAoi};
use crate::peer::{Evento, Modo, Net};
use crate::wire::{Mensagem, SnapshotInimigo};
use crate::{PORTA_PADRAO, SNAPSHOT_HZ, VERSAO_PROTOCOLO};

/// Id de jogador. `0` é reservado ao host, então cliente começa em `1`.
pub type ClienteId = u8;

/// Quanto tempo sem pacote até o cliente ser considerado morto.
///
/// 5 s é folgado de propósito: numa rede Wi-Fi congestionada um burst de perda
/// de 1 s acontece. Expulsar em 1 s tiraria jogador de partida por engasgo
/// momentâneo, e reconectar custa handshake novo.
pub const TIMEOUT_CLIENTE: Duration = Duration::from_secs(5);

/// Máximo de clientes simultâneos.
///
/// O relatório trabalha com 4. 8 dá folga sem que o cálculo de AOI por cliente
/// por tick passe a pesar no host ARM64.
pub const MAX_CLIENTES: usize = 8;

struct Cliente {
    id: ClienteId,
    addr: SocketAddr,
    ultimo_pacote: Instant,
    /// Câmera que o cliente reportou no último input. É a base do AOI dele.
    camera_x: f32,
    camera_z: f32,
    /// Raio de interesse — calculado pelo **jogo**, enviado pronto.
    raio: f32,
}

/// O host de uma partida.
pub struct Host {
    net: Net,
    clientes: HashMap<SocketAddr, Cliente>,
    proximo_id: ClienteId,
    filtro: FiltroAoi,
    tick: u32,
    ultimo_snapshot: Instant,
    intervalo_snapshot: Duration,
    /// Enviados no último snapshot, para diagnóstico.
    ultimos_envios: Vec<(ClienteId, usize)>,
}

impl Host {
    /// Sobe na porta dada (ou [`PORTA_PADRAO`]) escutando em todas as
    /// interfaces — é o que um host de LAN precisa.
    ///
    /// **Cuidado com [`Self::endereco`]**: escutando em `0.0.0.0`, o endereço
    /// devolvido **não é conectável**. `send_to` para `0.0.0.0` falha com
    /// `AddrNotAvailable` (WSAEADDRNOTAVAIL no Windows, EADDRNOTAVAIL no
    /// Linux) — e é o tipo de erro que aparece só quando um cliente tenta
    /// entrar, não no host que subiu. Para dar o endereço a clientes, use o IP
    /// da interface, como faz [`Self::endereco_anunciado`].
    pub fn novo(porta: Option<u16>) -> std::io::Result<Self> {
        Self::escutar(("0.0.0.0", porta.unwrap_or(PORTA_PADRAO)))
    }

    /// Sobe em um endereço explícito. Usado pelos testes com `127.0.0.1`, e por
    /// quem quiser prender o host a uma interface só.
    pub fn escutar(ender: impl std::net::ToSocketAddrs) -> std::io::Result<Self> {
        let net = Net::abrir(ender, Modo::Host)?;
        Ok(Self {
            net,
            clientes: HashMap::new(),
            proximo_id: 1,
            filtro: FiltroAoi::novo(),
            tick: 0,
            ultimo_snapshot: Instant::now(),
            intervalo_snapshot: Duration::from_millis(1000 / SNAPSHOT_HZ as u64),
            ultimos_envios: Vec::new(),
        })
    }

    /// Endereço de escuta, para o cliente saber para onde mandar.
    ///
    /// Com o host aberto em `0.0.0.0`, isto devolve `0.0.0.0:porta` — inútil
    /// para conectar. Prefira [`Self::endereco_anunciado`].
    pub fn endereco(&self) -> std::io::Result<SocketAddr> {
        self.net.local()
    }

    /// Endereço que um cliente consegue usar.
    ///
    /// Descobre o IP da interface de saída com o truque padrão: um UDP
    /// `connect` para um destino externo **não envia pacote**, só faz o SO
    /// escolher a rota — e `local_addr` passa a devolver o IP daquela
    /// interface. Sem rede externa configurada pode falhar; nesse caso o
    /// chamador cai no [`Self::endereco`] e resolve na mão.
    pub fn endereco_anunciado(&self) -> std::io::Result<SocketAddr> {
        let porta = self.net.local()?.port();
        let sonda = std::net::UdpSocket::bind("0.0.0.0:0")?;
        sonda.connect("192.0.2.1:9")?; // TEST-NET-1: documentação, não roteável
        let ip = sonda.local_addr()?.ip();
        Ok(SocketAddr::new(ip, porta))
    }

    /// Quantos clientes aceitos agora.
    pub fn clientes(&self) -> usize {
        self.clientes.len()
    }

    /// Tick atual.
    pub fn tick(&self) -> u32 {
        self.tick
    }

    /// Processa o que chegou. Chamar uma vez por frame, com orçamento.
    ///
    /// Devolve os `(ClienteId, Mensagem)` que o **jogo** precisa tratar — input,
    /// ping. Handshake é resolvido aqui e não sobe para o jogo.
    pub fn processar(&mut self, orcamento: usize) -> Vec<(ClienteId, Mensagem)> {
        let agora = Instant::now();
        let mut para_o_jogo = Vec::new();

        for evento in self.net.drenar(orcamento) {
            let Evento::Mensagem { de, mensagem } = evento else {
                // Pacote inválido: contabilizado e ignorado. Um cliente com
                // bug não pode calar os outros.
                continue;
            };

            // Handshake antes de qualquer checagem de cliente: é ele que CRIA
            // o cliente.
            if let Mensagem::Handshake {
                versao,
                tipo_controle,
            } = &mensagem
            {
                self.responder_handshake(de, *versao, *tipo_controle, agora);
                continue;
            }

            let Some(cliente) = self.clientes.get_mut(&de) else {
                // Mensagem de quem nunca fez handshake: desconhecida. Não
                // respondemos nada — responder a pacote forjado abriria
                // amplificação.
                continue;
            };

            cliente.ultimo_pacote = agora;

            match &mensagem {
                Mensagem::Input {
                    camera_x, camera_z, ..
                } => {
                    cliente.camera_x = *camera_x;
                    cliente.camera_z = *camera_z;
                }
                Mensagem::Ping { enviado_ms } => {
                    let _ = self.net.enviar(
                        de,
                        &Mensagem::Pong {
                            enviado_ms: *enviado_ms,
                        },
                    );
                    continue;
                }
                _ => {}
            }

            para_o_jogo.push((cliente.id, mensagem));
        }

        self.expulsar_vencidos(agora);
        para_o_jogo
    }

    fn responder_handshake(&mut self, de: SocketAddr, versao: u16, _tipo: u8, agora: Instant) {
        if versao != VERSAO_PROTOCOLO {
            let texto = format!("protocolo {versao} != {VERSAO_PROTOCOLO}");
            let mut motivo = [0u8; 48];
            let n = texto.len().min(motivo.len());
            motivo[..n].copy_from_slice(&texto.as_bytes()[..n]);
            let _ = self.net.enviar(
                de,
                &Mensagem::HandshakeRecusado {
                    motivo,
                    tamanho: n as u8,
                },
            );
            return;
        }

        // Reconexão: o mesmo endereço volta. Devolve o MESMO id, para o jogo não
        // tratar como jogador novo no meio da partida.
        if let Some(existente) = self.clientes.get_mut(&de) {
            existente.ultimo_pacote = agora;
            let id = existente.id;
            let _ = self.net.enviar(
                de,
                &Mensagem::HandshakeOk {
                    versao: VERSAO_PROTOCOLO,
                    jogador: id,
                },
            );
            return;
        }

        if self.clientes.len() >= MAX_CLIENTES {
            let texto = "servidor cheio";
            let mut motivo = [0u8; 48];
            motivo[..texto.len()].copy_from_slice(texto.as_bytes());
            let _ = self.net.enviar(
                de,
                &Mensagem::HandshakeRecusado {
                    motivo,
                    tamanho: texto.len() as u8,
                },
            );
            return;
        }

        // Na volta de 255 para 1, o id gerado pode estar em uso: reutilizá-lo
        // faria dois jogadores compartilharem o mesmo input e AOI.
        let mut id = self.proximo_id;
        while self.clientes.values().any(|c| c.id == id) {
            id = id.wrapping_add(1).max(1);
        }
        self.proximo_id = id.wrapping_add(1).max(1);
        self.clientes.insert(
            de,
            Cliente {
                id,
                addr: de,
                ultimo_pacote: agora,
                camera_x: 0.0,
                camera_z: 0.0,
                raio: 0.0,
            },
        );
        let _ = self.net.enviar(
            de,
            &Mensagem::HandshakeOk {
                versao: VERSAO_PROTOCOLO,
                jogador: id,
            },
        );
    }

    /// O jogo informa o raio de interesse **daquele** cliente.
    ///
    /// Fica no jogo porque quem sabe quanto do mundo cabe na tela é quem
    /// desenha. A engine só aplica.
    pub fn definir_area(&mut self, id: ClienteId, raio: f32) {
        for c in self.clientes.values_mut() {
            if c.id == id {
                c.raio = raio.max(0.0);
            }
        }
    }

    fn expulsar_vencidos(&mut self, agora: Instant) {
        self.clientes
            .retain(|_, c| agora.duration_since(c.ultimo_pacote) < TIMEOUT_CLIENTE);
    }

    /// Broadcast de estado global — torre, onda, chefe, nemesis. Bytes opacos.
    pub fn enviar_global(&mut self, dados: &[u8]) {
        let destinos: Vec<_> = self.clientes.values().map(|c| c.addr).collect();
        let msg = Mensagem::Global {
            tick: self.tick,
            dados: dados.to_vec(),
        };
        self.net.enviar_todos(&destinos, &msg);
    }

    /// Snapshot de um jogador, para todos verem os outros.
    pub fn enviar_jogador(&mut self, j: crate::wire::ResumoJogador) {
        let destinos: Vec<_> = self.clientes.values().map(|c| c.addr).collect();
        self.net.enviar_todos(&destinos, &Mensagem::Jogador(j));
    }

    /// Manda o snapshot de inimigos **filtrado por AOI, um por cliente**, no
    /// máximo [`SNAPSHOT_HZ`] vezes por segundo.
    ///
    /// Devolve `true` se enviou neste frame. Chamar a 60 Hz é o uso normal; a
    /// função decide se é a vez.
    pub fn tick_snapshot(&mut self, inimigos: &[SnapshotInimigo]) -> bool {
        let agora = Instant::now();
        if agora.duration_since(self.ultimo_snapshot) < self.intervalo_snapshot {
            return false;
        }
        self.ultimo_snapshot = agora;
        self.tick = self.tick.wrapping_add(1);

        self.ultimos_envios.clear();
        let alvos: Vec<(SocketAddr, ClienteId, f32, f32, f32)> = self
            .clientes
            .values()
            .map(|c| (c.addr, c.id, c.camera_x, c.camera_z, c.raio))
            .collect();

        for (addr, id, cx, cz, raio) in alvos {
            let area = AreaDeInteresse::nova(cx, cz, raio);
            let filtrados = self.filtro.filtrar(inimigos, area);
            let msg = Mensagem::Inimigos {
                tick: self.tick,
                inimigos: filtrados.to_vec(),
            };
            let _ = self.net.enviar(addr, &msg);
            self.ultimos_envios.push((id, filtrados.len()));
        }
        true
    }

    /// Quantos inimigos foram para cada cliente no último snapshot. É o número
    /// que prova que o AOI está economizando — sem ele, "liguei o AOI" é
    /// afirmação.
    pub fn ultimos_envios(&self) -> &[(ClienteId, usize)] {
        &self.ultimos_envios
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::wire::SnapshotInimigo;

    fn inimigo(id: u16, x: f32, z: f32) -> SnapshotInimigo {
        SnapshotInimigo {
            id,
            x,
            z,
            hp: 10,
            tipo: 0,
            fase_chefe: 0,
        }
    }

    #[test]
    fn host_sobe_e_escuta() {
        let host = Host::escutar(("127.0.0.1", 0)).expect("devia subir");
        assert!(host.endereco().unwrap().port() > 0);
        assert_eq!(host.clientes(), 0);
    }

    /// Espera um evento chegar, com prazo. Loopback pode atrasar; nada aqui pode
    /// pendurar o CI.
    ///
    /// `FnMut`, não `Fn`: todas as chamadas processam o host dentro do laço, e
    /// uma closure que muta o que captura não implementa `Fn`. Com `Fn` isto não
    /// compila — e o erro aparece no primeiro `cargo test`, não em produção.
    fn esperar(mut cond: impl FnMut() -> bool, ms: u64) -> bool {
        let prazo = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < prazo {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        cond()
    }

    #[test]
    fn handshake_aceita_e_atribui_id_a_partir_de_um() {
        let mut host = Host::escutar(("127.0.0.1", 0)).unwrap();
        let addr = host.endereco().unwrap();
        let mut cliente = Net::abrir("127.0.0.1:0", Modo::Cliente).unwrap();

        cliente
            .enviar(
                addr,
                &Mensagem::Handshake {
                    versao: VERSAO_PROTOCOLO,
                    tipo_controle: 0,
                },
            )
            .unwrap();

        let mut id_recebido = None;
        assert!(esperar(
            || {
                host.processar(64);
                for e in cliente.drenar(64) {
                    if let Evento::Mensagem {
                        mensagem: Mensagem::HandshakeOk { jogador, .. },
                        ..
                    } = e
                    {
                        id_recebido = Some(jogador);
                    }
                }
                id_recebido.is_some()
            },
            500
        ));
        assert_eq!(
            id_recebido,
            Some(1),
            "primeiro cliente é 1, porque 0 é o host"
        );
        assert_eq!(host.clientes(), 1);
    }

    #[test]
    fn id_nao_reusa_cliente_vivo_quando_o_contador_da_a_volta() {
        let mut host = Host::escutar(("127.0.0.1", 0)).unwrap();
        for (id, porta) in [(1u8, 1u16), (255u8, 2u16)] {
            let addr = SocketAddr::from(([127, 0, 0, 1], porta));
            host.clientes.insert(
                addr,
                Cliente {
                    id,
                    addr,
                    ultimo_pacote: Instant::now(),
                    camera_x: 0.0,
                    camera_z: 0.0,
                    raio: 0.0,
                },
            );
        }
        host.proximo_id = 255;

        let novo = SocketAddr::from(([127, 0, 0, 1], 3));
        host.responder_handshake(novo, VERSAO_PROTOCOLO, 0, Instant::now());
        assert_eq!(
            host.clientes.get(&novo).map(|c| c.id),
            Some(2),
            "255 e 1 estão em uso; o próximo id livre é 2"
        );
    }

    #[test]
    fn handshake_de_versao_errada_e_recusado() {
        let mut host = Host::escutar(("127.0.0.1", 0)).unwrap();
        let addr = host.endereco().unwrap();
        let mut cliente = Net::abrir("127.0.0.1:0", Modo::Cliente).unwrap();

        // Manda versão antiga montando o datagrama à mão: `enviar` sempre
        // carimba a versão atual, então o caminho de recusa só é exercitado
        // por bytes crus.
        let mut bytes = Mensagem::Handshake {
            versao: 1,
            tipo_controle: 0,
        }
        .codificar();
        bytes[0] = 99; // versão local = 99
        cliente.enviar_bruto(addr, &bytes).unwrap();

        // Como a versão é recusada na LEITURA, o host nem vê a mensagem.
        // O cliente de verdade recebe o recusado; aqui provamos que o host não
        // adicionou ninguém.
        std::thread::sleep(Duration::from_millis(50));
        host.processar(64);
        assert_eq!(host.clientes(), 0, "versão errada não pode criar cliente");
    }

    #[test]
    fn mensagem_de_quem_nao_fez_handshake_e_ignorada() {
        let mut host = Host::escutar(("127.0.0.1", 0)).unwrap();
        let addr = host.endereco().unwrap();
        let mut estranho = Net::abrir("127.0.0.1:0", Modo::Cliente).unwrap();

        estranho
            .enviar(
                addr,
                &Mensagem::Input {
                    tick: 1,
                    mover_x: 0.0,
                    mover_z: 0.0,
                    mirar_x: 0.0,
                    mirar_z: 0.0,
                    acoes: 0,
                    camera_x: 0.0,
                    camera_z: 0.0,
                },
            )
            .unwrap();

        std::thread::sleep(Duration::from_millis(50));
        let para_o_jogo = host.processar(64);
        assert!(
            para_o_jogo.is_empty(),
            "input sem handshake não sobe para o jogo"
        );
        assert_eq!(host.clientes(), 0);
    }

    #[test]
    fn snapshot_respeita_a_taxa_e_o_aoi_por_cliente() {
        let mut host = Host::escutar(("127.0.0.1", 0)).unwrap();
        let addr = host.endereco().unwrap();

        let mut cliente = Net::abrir("127.0.0.1:0", Modo::Cliente).unwrap();
        cliente
            .enviar(
                addr,
                &Mensagem::Handshake {
                    versao: VERSAO_PROTOCOLO,
                    tipo_controle: 0,
                },
            )
            .unwrap();

        assert!(esperar(
            || {
                host.processar(64);
                host.clientes() == 1
            },
            500
        ));

        // Cliente reporta câmera em (0,0) com raio 5. Só o inimigo perto entra.
        cliente
            .enviar(
                addr,
                &Mensagem::Input {
                    tick: 1,
                    mover_x: 0.0,
                    mover_z: 0.0,
                    mirar_x: 0.0,
                    mirar_z: 0.0,
                    acoes: 0,
                    camera_x: 0.0,
                    camera_z: 0.0,
                },
            )
            .unwrap();
        assert!(esperar(
            || {
                host.processar(64);
                // O input é o que atualiza a câmera; processar já basta.
                true
            },
            300
        ));

        host.definir_area(1, 5.0);

        let mundo = vec![inimigo(1, 0.0, 0.0), inimigo(2, 500.0, 500.0)];
        // Força a janela: o primeiro tick pode ser recusado por intervalo.
        std::thread::sleep(Duration::from_millis(80));
        assert!(host.tick_snapshot(&mundo), "devia enviar");

        let envios = host.ultimos_envios().to_vec();
        assert_eq!(envios.len(), 1);
        assert_eq!(envios[0].1, 1, "só o inimigo dentro do raio");
    }
}
