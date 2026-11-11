//! O cliente: quem manda input e recebe estado.
//!
//! ## O que o cliente NÃO faz, e por quê
//!
//! Não simula. Não prediz a própria posição. Não corrige o servidor.
//!
//! Predição exigiria simulação determinística dos dois lados — mesma física,
//! mesmo ponto flutuante, mesma ordem de iteração. Em ARM64 com `f32` isso é
//! possível, mas é a parte mais cara de um netcode e o relatório não pede:
//! o escopo é **LAN**, onde o RTT é 1–4 ms. Predição em LAN resolve um problema
//! que não existe e cria outro que ninguém consegue reproduzir.
//!
//! O que o cliente faz de verdade: manter o host vivo (ping), mandar input com
//! a câmera para o AOI, e entregar ao jogo o estado que chegou.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::peer::{Evento, Modo, Net};
use crate::wire::Mensagem;
use crate::VERSAO_PROTOCOLO;

/// Estado da conexão com o host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Estado {
    /// Handshake enviado, resposta ainda não chegou.
    Conectando,
    /// Host aceitou. Traz o id do jogador.
    Conectado(u8),
    /// Host recusou, ou a conexão caiu.
    Recusado,
}

/// Intervalo de ping. Serve para detectar host morto e para o RTT do HUD.
pub const INTERVALO_PING: Duration = Duration::from_secs(2);

/// Sem resposta do host por este tempo → considera caído.
///
/// 3 × o intervalo de ping: dois pings perdidos seguidos não derrubam a sessão,
/// três sim.
pub const TIMEOUT_HOST: Duration = Duration::from_secs(6);

/// O cliente de uma partida.
pub struct Cliente {
    net: Net,
    host: SocketAddr,
    estado: Estado,
    jogador: Option<u8>,
    ultimo_pacote: Instant,
    ultimo_ping: Instant,
    /// Referência de tempo monotônica do processo.
    ///
    /// Precisa existir: a marca que vai no `Ping` tem de ser comparável com a
    /// que volta no `Pong`. Usar `Instant::now().elapsed()` dentro da função
    /// daria sempre ~0 ms e o RTT sairia zero — número plausível e falso, que é
    /// pior que ausente.
    inicio: Instant,
    /// RTT medido no último `Pong`, em ms.
    rtt_ms: Option<u32>,
    /// Tick do último snapshot recebido, para o jogo saber se ficou velho.
    ultimo_tick: u32,
}

impl Cliente {
    /// Aponta para um host e manda o handshake.
    ///
    /// `endereco` aceita `"192.168.1.10:47721"` ou `("ip", porta)`.
    pub fn entrar(endereco: impl std::net::ToSocketAddrs) -> std::io::Result<Self> {
        let host = endereco.to_socket_addrs()?.next().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "endereço vazio")
        })?;

        let mut net = Net::abrir("0.0.0.0:0", Modo::Cliente)?;
        let agora = Instant::now();

        // O handshake sai já na criação: assim `estado` só precisa distinguir
        // "esperando resposta" de "respondido".
        net.enviar(
            host,
            &Mensagem::Handshake {
                versao: VERSAO_PROTOCOLO,
                tipo_controle: 0,
            },
        )?;

        Ok(Self {
            net,
            host,
            estado: Estado::Conectando,
            jogador: None,
            ultimo_pacote: agora,
            ultimo_ping: agora,
            inicio: agora,
            rtt_ms: None,
            ultimo_tick: 0,
        })
    }

    /// Endereço do host.
    pub fn host(&self) -> SocketAddr {
        self.host
    }

    /// Estado da conexão.
    pub fn estado(&self) -> Estado {
        self.estado
    }

    /// Id atribuído pelo host, quando conectado.
    pub fn jogador(&self) -> Option<u8> {
        self.jogador
    }

    /// RTT do último ping, em ms.
    pub fn rtt_ms(&self) -> Option<u32> {
        self.rtt_ms
    }

    /// Tick do último snapshot.
    pub fn ultimo_tick(&self) -> u32 {
        self.ultimo_tick
    }

    /// Manda o input do jogador, com a câmera que o host usa para o AOI.
    ///
    /// Chamar a 60 Hz é normal; o intervalo de envio é decisão do jogo (input
    /// 60 Hz com snapshot 15 Hz é o arranjo usual).
    pub fn enviar_input(&mut self, input: Mensagem) -> std::io::Result<()> {
        if !matches!(
            input,
            Mensagem::Input { .. } | Mensagem::ShopBuy { .. } | Mensagem::Ping { .. }
        ) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cliente só manda Input, ShopBuy ou Ping",
            ));
        }
        self.net.enviar(self.host, &input)?;
        Ok(())
    }

    /// Processa o que chegou e devolve o que é do **jogo**.
    ///
    /// Handshake e pong são resolvidos aqui: o jogo não tem o que fazer com
    /// eles, e subir esse detalhe obrigaria cada jogo a reimplementar.
    pub fn processar(&mut self, orcamento: usize) -> Vec<Mensagem> {
        let agora = Instant::now();
        let mut para_o_jogo = Vec::new();

        for evento in self.net.drenar(orcamento) {
            let Evento::Mensagem { de, mensagem } = evento else {
                continue;
            };
            // Só o host fala com o cliente. Pacote de outro endereço é ruído
            // (ou tentativa de injeção) e não muda estado.
            if de != self.host {
                continue;
            }
            self.ultimo_pacote = agora;

            match &mensagem {
                Mensagem::HandshakeOk { versao, jogador } => {
                    if *versao != VERSAO_PROTOCOLO {
                        self.estado = Estado::Recusado;
                        continue;
                    }
                    self.jogador = Some(*jogador);
                    self.estado = Estado::Conectado(*jogador);
                }
                Mensagem::HandshakeRecusado { .. } => {
                    self.estado = Estado::Recusado;
                }
                Mensagem::Pong { enviado_ms } => {
                    // Marca contra `inicio`, a mesma base usada no envio do
                    // `Ping`. `saturating_sub` porque ajuste de relógio no meio
                    // da partida não pode estourar o `u32`.
                    let agora_ms = self.inicio.elapsed().as_millis() as u32;
                    self.rtt_ms = Some(saturating_rtt(agora_ms, *enviado_ms));
                    continue;
                }
                Mensagem::Inimigos { tick, .. } => {
                    self.ultimo_tick = *tick;
                    para_o_jogo.push(mensagem);
                }
                _ => para_o_jogo.push(mensagem),
            }
        }

        // Ping periódico. Sai depois do dreno para não competir com o input.
        if agora.duration_since(self.ultimo_ping) >= INTERVALO_PING {
            self.ultimo_ping = agora;
            let ms = self.inicio.elapsed().as_millis() as u32;
            let _ = self
                .net
                .enviar(self.host, &Mensagem::Ping { enviado_ms: ms });
        }

        if agora.duration_since(self.ultimo_pacote) > TIMEOUT_HOST
            && matches!(self.estado, Estado::Conectado(_))
        {
            self.estado = Estado::Recusado;
        }

        para_o_jogo
    }

    /// Contadores para diagnóstico.
    pub fn contadores(&self) -> (u64, u64, u64) {
        self.net.contadores()
    }
}

/// RTT a partir de duas marcas em ms, sem estourar.
fn saturating_rtt(agora_ms: u32, enviado_ms: u32) -> u32 {
    agora_ms.saturating_sub(enviado_ms)
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::host::Host;
    use crate::wire::SnapshotInimigo;

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
    fn cliente_entra_e_recebe_id() {
        let mut host = Host::novo(Some(0)).unwrap();
        let addr = host.endereco().unwrap();

        let mut cliente = Cliente::entrar(addr).expect("devia entrar");
        assert_eq!(cliente.estado(), Estado::Conectando);

        assert!(esperar(
            || {
                host.processar(64);
                cliente.processar(64);
                cliente.estado() != Estado::Conectando
            },
            800
        ));
        match cliente.estado() {
            Estado::Conectado(id) => assert_eq!(id, 1),
            outro => panic!("esperava Conectado, veio {outro:?}"),
        }
        assert_eq!(cliente.jogador(), Some(1));
    }

    #[test]
    fn input_chega_ao_host_e_snapshot_volta() {
        let mut host = Host::novo(Some(0)).unwrap();
        let addr = host.endereco().unwrap();
        let mut cliente = Cliente::entrar(addr).unwrap();

        assert!(esperar(
            || {
                host.processar(64);
                cliente.processar(64);
                matches!(cliente.estado(), Estado::Conectado(_))
            },
            800
        ));

        cliente
            .enviar_input(Mensagem::Input {
                tick: 1,
                mover_x: 1.0,
                mover_z: 0.0,
                mirar_x: 0.0,
                mirar_z: 0.0,
                acoes: 0,
                camera_x: 100.0,
                camera_z: 100.0,
            })
            .unwrap();

        let mut recebeu_input = false;
        assert!(esperar(
            || {
                for (_, m) in host.processar(64) {
                    if matches!(m, Mensagem::Input { .. }) {
                        recebeu_input = true;
                    }
                }
                recebeu_input
            },
            800
        ));

        // Cliente está em (100,100) com raio 10: só o inimigo de lá entra.
        host.definir_area(1, 10.0);
        let mundo = vec![
            SnapshotInimigo {
                id: 1,
                x: 100.0,
                z: 100.0,
                hp: 10,
                tipo: 0,
                fase_chefe: 0,
            },
            SnapshotInimigo {
                id: 2,
                x: 0.0,
                z: 0.0,
                hp: 10,
                tipo: 0,
                fase_chefe: 0,
            },
        ];

        std::thread::sleep(Duration::from_millis(80));
        assert!(host.tick_snapshot(&mundo));

        let mut recebeu = None;
        assert!(esperar(
            || {
                for m in cliente.processar(64) {
                    if let Mensagem::Inimigos { inimigos, .. } = m {
                        recebeu = Some(inimigos);
                    }
                }
                recebeu.is_some()
            },
            800
        ));
        let inimigos = recebeu.unwrap();
        assert_eq!(inimigos.len(), 1, "AOI devia cortar o inimigo distante");
        assert_eq!(inimigos[0].id, 1);
    }

    #[test]
    fn enviar_input_recusa_mensagem_que_nao_e_do_cliente() {
        let host = Host::novo(Some(0)).unwrap();
        let addr = host.endereco().unwrap();
        let mut cliente = Cliente::entrar(addr).unwrap();

        let erro = cliente
            .enviar_input(Mensagem::HandshakeOk {
                versao: VERSAO_PROTOCOLO,
                jogador: 1,
            })
            .unwrap_err();
        assert_eq!(erro.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn pacote_de_host_desconhecido_e_ignorado() {
        let host = Host::novo(Some(0)).unwrap();
        let addr = host.endereco().unwrap();
        let mut cliente = Cliente::entrar(addr).unwrap();

        // Um segundo host manda HandshakeOk para o cliente; o endereço não é o
        // do host combinado, então tem de ser ignorado.
        let mut impostor = crate::peer::Net::abrir("127.0.0.1:0", crate::peer::Modo::Host).unwrap();
        let cliente_addr = cliente.net.local().unwrap();
        impostor
            .enviar(
                cliente_addr,
                &Mensagem::HandshakeOk {
                    versao: VERSAO_PROTOCOLO,
                    jogador: 99,
                },
            )
            .unwrap();

        std::thread::sleep(Duration::from_millis(50));
        cliente.processar(64);
        assert_ne!(
            cliente.jogador(),
            Some(99),
            "pacote de outro endereço não vale"
        );
    }

    #[test]
    fn rtt_satura_em_vez_de_estourar() {
        assert_eq!(saturating_rtt(100, 60), 40);
        assert_eq!(saturating_rtt(10, 60), 0, "relógio para trás não estoura");
    }
}
