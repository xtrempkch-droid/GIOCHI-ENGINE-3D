//! O socket e o laço de recebimento — o ponto onde "não bloquear" se prova.
//!
//! ## A regra que este módulo existe para garantir
//!
//! O timestep da engine é fixo a 60 Hz. Se uma leitura de socket bloquear
//! esperando pacote, o frame inteiro atrasa — e num handheld isso aparece como
//! engasgo cuja causa não se acha, porque o código de rede está numa função
//! chamada "poll".
//!
//! Duas defesas, as duas usadas:
//!
//! 1. o socket é criado **não-bloqueante** (`set_nonblocking(true)`), então
//!    `recv_from` devolve `WouldBlock` em vez de esperar;
//! 2. [`Net::drenar`] recebe um **orçamento** de pacotes. Sem teto, um flood de
//!    10 mil datagramas consumiria o frame inteiro processando-os — que é a
//!    mesma falha por outro caminho.

use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

use crate::wire::{ErroLeitura, Mensagem};

/// Papel deste nó.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modo {
    /// Simula o jogo e aceita clientes.
    Host,
    /// Manda input e recebe snapshots.
    Cliente,
}

/// Algo que chegou da rede.
#[derive(Debug, Clone, PartialEq)]
pub enum Evento {
    /// Pacote válido de um peer conhecido ou novo.
    Mensagem {
        /// Quem mandou.
        de: SocketAddr,
        /// O que mandou.
        mensagem: Mensagem,
    },
    /// Pacote que não decodificou. **Não derruba a conexão**: um peer com bug
    /// não pode calar os outros, e o host prefere perder um datagrama a reiniciar
    /// a partida.
    Invalido {
        /// Quem mandou.
        de: SocketAddr,
        /// Por que foi recusado.
        erro: ErroLeitura,
    },
}

/// O socket de rede, em modo não-bloqueante.
pub struct Net {
    socket: UdpSocket,
    modo: Modo,
    recebidos: u64,
    invalidos: u64,
    enviados: u64,
}

impl Net {
    /// Abre num endereço. Para o host é a porta de escuta; para o cliente,
    /// `0.0.0.0:0` costuma ser o certo (porta efêmera).
    pub fn abrir(ender: impl ToSocketAddrs, modo: Modo) -> io::Result<Self> {
        let socket = UdpSocket::bind(ender)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            modo,
            recebidos: 0,
            invalidos: 0,
            enviados: 0,
        })
    }

    /// Abre na porta padrão escolhendo o endereço pelo modo.
    pub fn aberto_padrao(modo: Modo, porta: u16) -> io::Result<Self> {
        match modo {
            Modo::Host => Self::abrir(("0.0.0.0", porta), modo),
            Modo::Cliente => Self::abrir(("0.0.0.0", 0), modo),
        }
    }

    /// Papel deste nó.
    pub fn modo(&self) -> Modo {
        self.modo
    }

    /// Endereço local efetivo (o do cliente já traz a porta dada pelo SO).
    pub fn local(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Envia uma mensagem. `WouldBlock` é possível com a fila de transmissão
    /// cheia — por isso o retorno é `io::Result`, e não `()`.
    pub fn enviar(&mut self, para: SocketAddr, mensagem: &Mensagem) -> io::Result<usize> {
        let bytes = mensagem.codificar();
        let n = self.socket.send_to(&bytes, para)?;
        self.enviados += 1;
        Ok(n)
    }

    /// Envia para todos, ignorando quem falhou. Usado no broadcast de estado
    /// global: um cliente caído não pode impedir os outros de receber.
    pub fn enviar_todos(&mut self, destinos: &[SocketAddr], mensagem: &Mensagem) {
        for d in destinos {
            let _ = self.enviar(*d, mensagem);
        }
    }

    /// Envia bytes crus, sem cabeçalho de versão.
    ///
    /// Existe porque **testar o caminho de erro exige produzir erro**: um
    /// datagrama com versão trocada ou tag inválida não pode ser montado por
    /// [`Mensagem::codificar`], que sempre escreve o formato correto. Sem esta
    /// porta, os testes que provam "pacote ruim não derruba o host" teriam de
    /// acessar o campo privado — e um teste que fura o encapsulamento para
    /// funcionar está testando outra coisa.
    ///
    /// Não use em produção para mandar [`Mensagem`]; para isso existe
    /// [`Self::enviar`], que carimba a versão.
    pub fn enviar_bruto(&mut self, para: SocketAddr, bytes: &[u8]) -> io::Result<usize> {
        let n = self.socket.send_to(bytes, para)?;
        self.enviados += 1;
        Ok(n)
    }

    /// Drena até `orcamento` pacotes. Devolve os eventos na ordem em que
    /// chegaram.
    ///
    /// O orçamento é o que garante que o frame não seja consumido por rede: com
    /// `orcamento = 64` e 60 Hz, o pior caso é 3840 datagramas por segundo sem
    /// nenhum bloqueio.
    pub fn drenar(&mut self, orcamento: usize) -> Vec<Evento> {
        let mut eventos = Vec::new();
        let mut buffer = [0u8; 2048];

        for _ in 0..orcamento {
            match self.socket.recv_from(&mut buffer) {
                Ok((n, de)) => {
                    self.recebidos += 1;
                    match Mensagem::decodificar(&buffer[..n]) {
                        Ok(mensagem) => eventos.push(Evento::Mensagem { de, mensagem }),
                        Err(erro) => {
                            self.invalidos += 1;
                            eventos.push(Evento::Invalido { de, erro });
                        }
                    }
                }
                // Sem pacote agora: é o caso normal 99 % do tempo.
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                // ICMP "porta inalcançável" chega aqui no Linux e é constante
                // quando um cliente sai. Ignorar em vez de propagar: o peer
                // some do mapa por timeout, não por erro de socket.
                Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => continue,
                Err(_) => break,
            }
        }
        eventos
    }

    /// Contadores para diagnóstico (log de HUD, `--diag`).
    pub fn contadores(&self) -> (u64, u64, u64) {
        (self.recebidos, self.invalidos, self.enviados)
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn abrir_em_porta_efemera_nao_falha() {
        let net = Net::abrir("127.0.0.1:0", Modo::Cliente).expect("devia abrir");
        assert!(net.local().unwrap().port() > 0);
    }

    #[test]
    fn socket_e_nao_bloqueante_de_verdade() {
        // Antes existia um método `nao_bloqueante()` que devolvia `true` sempre
        // — ele não conseguia LER o flag, só reescrevia. Um teste baseado nele
        // não provaria nada. A prova real é comportamental: sem pacote na fila,
        // drenar tem de voltar na hora. É o que o teste abaixo mede.
        let mut net = Net::abrir("127.0.0.1:0", Modo::Host).unwrap();
        let inicio = std::time::Instant::now();
        let eventos = net.drenar(64);
        assert!(eventos.is_empty());
        assert!(
            inicio.elapsed() < std::time::Duration::from_millis(50),
            "drenar bloqueou por {:?} — o socket não está em modo não-bloqueante",
            inicio.elapsed()
        );
    }

    #[test]
    fn drenar_sem_pacote_devolve_vazio_na_hora() {
        // O teste que importa: com socket não-bloqueante e nada na fila, a
        // chamada tem de retornar imediatamente. Se bloqueasse, este teste
        // penduraria — que é exatamente o sintoma que o módulo evita.
        let mut net = Net::abrir("127.0.0.1:0", Modo::Host).unwrap();
        let inicio = std::time::Instant::now();
        let eventos = net.drenar(64);
        assert!(eventos.is_empty());
        assert!(
            inicio.elapsed() < std::time::Duration::from_millis(50),
            "drenar bloqueou por {:?}",
            inicio.elapsed()
        );
    }

    #[test]
    fn envia_e_recebe_entre_dois_sockets() {
        let host = Net::abrir("127.0.0.1:0", Modo::Host).unwrap();
        let host_addr = host.local().unwrap();
        let mut cliente = Net::abrir("127.0.0.1:0", Modo::Cliente).unwrap();
        let mut host = host;

        cliente
            .enviar(host_addr, &Mensagem::Ping { enviado_ms: 42 })
            .expect("devia enviar");

        // Um datagrama loopback pode levar alguns microssegundos para aparecer.
        // Espera curta e limitada: se não chegar em 200 ms, é bug.
        let mut achou = false;
        let prazo = std::time::Instant::now() + std::time::Duration::from_millis(200);
        while std::time::Instant::now() < prazo {
            for e in host.drenar(64) {
                if let Evento::Mensagem { mensagem, .. } = e {
                    assert_eq!(mensagem, Mensagem::Ping { enviado_ms: 42 });
                    achou = true;
                }
            }
            if achou {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(achou, "o ping não chegou");
    }

    #[test]
    fn pacote_invalido_vira_evento_e_nao_derruba_o_socket() {
        let mut host = Net::abrir("127.0.0.1:0", Modo::Host).unwrap();
        let host_addr = host.local().unwrap();
        let cliente = Net::abrir("127.0.0.1:0", Modo::Cliente).unwrap();

        // Manda lixo direto pelo socket do cliente.
        cliente
            .socket
            .send_to(&[0xDE, 0xAD, 0xBE, 0xEF], host_addr)
            .unwrap();

        let mut viu_invalido = false;
        let prazo = std::time::Instant::now() + std::time::Duration::from_millis(200);
        while std::time::Instant::now() < prazo && !viu_invalido {
            for e in host.drenar(64) {
                if let Evento::Invalido { erro, .. } = e {
                    assert!(matches!(erro, ErroLeitura::VersaoIncompativel { .. }));
                    viu_invalido = true;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(viu_invalido, "lixo devia virar Evento::Invalido");

        // E o socket continua utilizável depois do lixo.
        assert!(host.local().is_ok());
    }

    #[test]
    fn orcamento_limita_quantos_pacotes_por_chamada() {
        let mut host = Net::abrir("127.0.0.1:0", Modo::Host).unwrap();
        let host_addr = host.local().unwrap();
        let mut cliente = Net::abrir("127.0.0.1:0", Modo::Cliente).unwrap();

        for i in 0..20 {
            cliente
                .enviar(host_addr, &Mensagem::Ping { enviado_ms: i })
                .unwrap();
        }

        // Orçamento de 5: no máximo 5 por chamada, mesmo com 20 na fila.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let primeira = host.drenar(5);
        assert!(primeira.len() <= 5, "veio {}", primeira.len());
    }
}
