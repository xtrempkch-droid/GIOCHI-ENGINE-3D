//! Transporte sobre [`laminar`], com as garantias de entrega que o protocolo
//! próprio não tem.
//!
//! ## Por que os dois existem
//!
//! O transporte próprio ([`crate::peer`]) é um `UdpSocket` sem garantia nenhuma:
//! datagrama que se perde, se perde. Isso basta para snapshot de posição — o
//! próximo tick corrige — e **não** basta para handshake, compra na loja ou
//! estado global: perder um `ShopBuy` é uma compra que o jogador fez e não
//! aconteceu.
//!
//! O [`laminar`] é a camada de rede do Amethyst, usada em jogos: dá canais
//! confiáveis-ordenados, sequenciados, fragmentação e RTT, sobre UDP, **sem
//! runtime assíncrono** — que é o requisito deste projeto (timestep fixo,
//! Cortex-A55).
//!
//! Aqui o `laminar` **não substitui** o protocolo: ele transporta os mesmos
//! bytes de [`crate::wire`]. O que muda é a garantia por variante de mensagem.
//!
//! ## O que este módulo NÃO faz
//!
//! Não é um segundo protocolo. A serialização continua sendo a nossa, e
//! `VERSAO_PROTOCOLO` continua sendo a única checagem de compatibilidade.
//! Trocar de transporte não muda o que trafega.
//!
//! ## Estado
//!
//! Ativa com a feature `laminar`. O transporte próprio continua o padrão, para
//! o alvo aarch64 poder ficar sem dependência nenhuma quando for preciso.

use std::io;
use std::net::SocketAddr;
use std::time::Instant;

use laminar::{Config, Packet, Socket, SocketEvent};

use crate::peer::{Evento, Modo};
use crate::wire::{ErroLeitura, Mensagem};

/// Garantia de entrega de uma mensagem.
///
/// A escolha não é preferência: cada variante tem uma consequência diferente se
/// o pacote sumir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Garantia {
    /// Não se perde. Para o que muda estado e o jogador percebe.
    Confiavel,
    /// Pode se perder, mas o que chega depois nunca é mais velho que o anterior.
    /// Para input: input atrasado é pior que input perdido.
    Sequenciada,
    /// Pode se perder. Para snapshot: o próximo tick substitui.
    Descartavel,
}

/// Canal (stream) do laminar. Ordenação é por canal, não global.
///
/// Separar em canais evita que um snapshot grande atrase um `ShopBuy`.
mod canal {
    /// Handshake, estado global, compra.
    pub const CONTROLE: u8 = 0;
    /// Input e posição de jogador.
    pub const ENTRADA: u8 = 1;
}

/// A garantia que cada variante recebe, e por quê.
///
/// Documentada aqui porque é a decisão que muda comportamento em rede ruim, e
/// não é visível no resto do código.
pub fn garantia_de(mensagem: &Mensagem) -> Garantia {
    match mensagem {
        // Perder o handshake é não entrar na partida.
        Mensagem::Handshake { .. }
        | Mensagem::HandshakeOk { .. }
        | Mensagem::HandshakeRecusado { .. } => Garantia::Confiavel,

        // Compra que se perde é compra que não aconteceu, com o ouro já gasto
        // do lado do cliente.
        Mensagem::ShopBuy { .. } => Garantia::Confiavel,

        // Onda, torre, chefe, nemesis: se o cliente perde, ele mostra estado
        // velho até o próximo — e o HUD mentiria sobre o HP da torre.
        Mensagem::Global { .. } => Garantia::Confiavel,

        // Input atrasado não serve para nada: o host simula o presente. Ordem
        // importa dentro do fluxo, então sequenciada e não descartável pura.
        Mensagem::Input { .. } => Garantia::Sequenciada,

        // Posição de jogador é interpolada no cliente; perder um quadro é
        // invisível, mas chegar fora de ordem faz o jogador "voltar".
        Mensagem::Jogador(_) => Garantia::Sequenciada,

        // Snapshot: o próximo substitui inteiro. Confiável aqui só gastaria
        // banda reenviando posição que já ficou velha.
        Mensagem::Inimigos { .. } => Garantia::Descartavel,

        // RTT: perder um ping só atrasa a medição.
        Mensagem::Ping { .. } | Mensagem::Pong { .. } => Garantia::Descartavel,
    }
}

/// Transporte laminar.
pub struct Transporte {
    socket: Socket,
    modo: Modo,
    recebidos: u64,
    invalidos: u64,
    enviados: u64,
    conexoes: u64,
}

impl Transporte {
    /// Abre num endereço. `blocking_mode: false` é forçado: o laço da engine tem
    /// timestep fixo e não pode esperar pacote.
    pub fn abrir(ender: impl std::net::ToSocketAddrs, modo: Modo) -> io::Result<Self> {
        let config = Config {
            blocking_mode: false,
            ..Config::default()
        };
        let socket = Socket::bind_with_config(ender, config)
            .map_err(|e| io::Error::other(format!("laminar: {e}")))?;
        Ok(Self {
            socket,
            modo,
            recebidos: 0,
            invalidos: 0,
            enviados: 0,
            conexoes: 0,
        })
    }

    /// Endereço local efetivo.
    pub fn local(&self) -> io::Result<SocketAddr> {
        self.socket
            .local_addr()
            .map_err(|e| io::Error::other(format!("laminar: {e}")))
    }

    /// Papel deste nó.
    pub fn modo(&self) -> Modo {
        self.modo
    }

    /// Envia com a garantia que a variante exige (ver [`garantia_de`]).
    pub fn enviar(&mut self, para: SocketAddr, mensagem: &Mensagem) -> io::Result<usize> {
        let bytes = mensagem.codificar();
        let n = bytes.len();
        let pacote = match garantia_de(mensagem) {
            // Todo o tráfego confiável vai no mesmo canal, e é isso que dá a
            // ordem que interessa: o `Global` que abre a loja chega antes do
            // `ShopBuy` que compra nela. Canais separados por tipo quebrariam
            // justamente essa ordem entre tipos diferentes.
            Garantia::Confiavel => Packet::reliable_ordered(para, bytes, Some(canal::CONTROLE)),
            Garantia::Sequenciada => Packet::reliable_sequenced(para, bytes, Some(canal::ENTRADA)),
            Garantia::Descartavel => Packet::unreliable(para, bytes),
        };
        self.socket
            .send(pacote)
            .map_err(|e| io::Error::other(format!("laminar: {e}")))?;
        self.enviados += 1;
        Ok(n)
    }

    /// Bombeia a rede e devolve o que chegou.
    ///
    /// `manual_poll` é obrigatório antes de `recv`: é ele que move os pacotes do
    /// sistema operacional para a fila do laminar.
    pub fn drenar(&mut self, orcamento: usize) -> Vec<Evento> {
        let agora = Instant::now();
        self.socket.manual_poll(agora);

        let mut eventos = Vec::new();
        for _ in 0..orcamento {
            let Some(evento) = self.socket.recv() else {
                break;
            };
            match evento {
                SocketEvent::Packet(pacote) => {
                    self.recebidos += 1;
                    let de = pacote.addr();
                    match Mensagem::decodificar(pacote.payload()) {
                        Ok(mensagem) => eventos.push(Evento::Mensagem { de, mensagem }),
                        Err(erro) => {
                            self.invalidos += 1;
                            eventos.push(Evento::Invalido { de, erro });
                        }
                    }
                }
                // O laminar fecha a conexão quando o peer fica calado; o host
                // já tem o próprio timeout, então aqui só se conta.
                SocketEvent::Connect(_) => {
                    self.conexoes += 1;
                }
                SocketEvent::Timeout(_) | SocketEvent::Disconnect(_) => {}
            }
        }
        eventos
    }

    /// Contadores para diagnóstico: `(recebidos, inválidos, enviados)`.
    pub fn contadores(&self) -> (u64, u64, u64) {
        (self.recebidos, self.invalidos, self.enviados)
    }

    /// Quantas conexões o laminar estabeleceu desde o início.
    pub fn conexoes(&self) -> u64 {
        self.conexoes
    }
}

/// Erro de decodificação exposto para quem trata evento, sem depender do tipo
/// interno do laminar.
pub type ErroDecodificacao = ErroLeitura;

#[cfg(test)]
mod testes {
    use super::*;
    use std::time::{Duration, Instant};

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
    fn cada_variante_tem_garantia_declarada() {
        // Se alguém acrescentar variante ao protocolo, este teste lembra de
        // decidir a garantia dela em vez de herdar a do `match` por acidente.
        assert_eq!(
            garantia_de(&Mensagem::Handshake {
                versao: 1,
                tipo_controle: 0
            }),
            Garantia::Confiavel
        );
        assert_eq!(
            garantia_de(&Mensagem::ShopBuy { item_id: 1 }),
            Garantia::Confiavel
        );
        assert_eq!(
            garantia_de(&Mensagem::Inimigos {
                tick: 1,
                inimigos: vec![]
            }),
            Garantia::Descartavel
        );
        assert_eq!(
            garantia_de(&Mensagem::Ping { enviado_ms: 0 }),
            Garantia::Descartavel
        );
        assert_eq!(
            garantia_de(&Mensagem::Input {
                tick: 0,
                mover_x: 0.0,
                mover_z: 0.0,
                mirar_x: 0.0,
                mirar_z: 0.0,
                acoes: 0,
                camera_x: 0.0,
                camera_z: 0.0
            }),
            Garantia::Sequenciada
        );
    }

    #[test]
    fn abre_em_porta_efemera() {
        let t = Transporte::abrir("127.0.0.1:0", Modo::Host).expect("devia abrir");
        assert!(t.local().unwrap().port() > 0);
    }

    #[test]
    fn mensagem_atravessa_dois_transportes() {
        let mut host = Transporte::abrir("127.0.0.1:0", Modo::Host).unwrap();
        let addr = host.local().unwrap();
        let mut cliente = Transporte::abrir("127.0.0.1:0", Modo::Cliente).unwrap();

        cliente
            .enviar(
                addr,
                &Mensagem::Handshake {
                    versao: crate::VERSAO_PROTOCOLO,
                    tipo_controle: 0,
                },
            )
            .unwrap();

        // `Socket::send` do laminar só ENFILEIRA. Quem transmite de verdade é o
        // `manual_poll`, que roda dentro de `drenar`. Um teste que só bombeia o
        // host nunca vê o pacote sair — e na engine isso significa que os dois
        // lados precisam chamar `drenar` a cada frame, não só quem recebe.
        let mut achou = None;
        assert!(esperar(
            || {
                cliente.drenar(64);
                for e in host.drenar(64) {
                    if let Evento::Mensagem { mensagem, .. } = e {
                        achou = Some(mensagem);
                    }
                }
                achou.is_some()
            },
            2000
        ));
        assert!(matches!(achou, Some(Mensagem::Handshake { .. })));
    }

    #[test]
    fn lixo_vira_evento_invalido_e_nao_panic() {
        // O laminar entrega payload opaco; quem valida é o nosso `wire`.
        let mut host = Transporte::abrir("127.0.0.1:0", Modo::Host).unwrap();
        let addr = host.local().unwrap();
        let mut cliente = Transporte::abrir("127.0.0.1:0", Modo::Cliente).unwrap();

        let pacote = Packet::unreliable(addr, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        cliente.socket.send(pacote).unwrap();

        let mut viu = false;
        assert!(esperar(
            || {
                cliente.drenar(64);
                for e in host.drenar(64) {
                    if matches!(e, Evento::Invalido { .. }) {
                        viu = true;
                    }
                }
                viu
            },
            2000
        ));
    }
}
