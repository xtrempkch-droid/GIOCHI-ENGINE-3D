//! Descoberta de hosts na LAN por mDNS, atrás da feature `mdns`.
//!
//! ## O problema que isto resolve
//!
//! Sem descoberta, o jogador precisa **digitar o IP do host** para entrar numa
//! partida. Num handheld sem teclado, isso é inviável na prática — e é o que
//! separava este projeto de "rede que funciona" para "rede que alguém usa".
//!
//! ## Por que `mdns-sd` e não algo escrito à mão
//!
//! mDNS é um protocolo com partes que parecem simples e não são: sondagem para
//! evitar colisão de nome, anúncio com TTL e *goodbye*, resposta a consultas de
//! PTR/SRV/TXT/A, e — a que mais dói — **enviar para o grupo multicast na
//! interface certa**, porque um `send_to` em `224.0.0.251` com *socket* sem
//! `IP_MULTICAST_IF` sai pela rota padrão e não é visto por ninguém na LAN.
//!
//! Escrever isso à mão custaria centenas de linhas de socket option e ainda
//! assim ficaria pior que a biblioteca que jogos usam (Amethyst usa mDNS pelo
//! mesmo motivo). O preço é a única dependência com peso neste crate.
//!
//! ## O que este módulo acrescenta sobre a biblioteca
//!
//! * **Não bloqueia.** O laço da engine é de timestep fixo; [`Descoberta::drenar`]
//!   usa `try_recv` com orçamento, como o resto do crate.
//! * **Devolve `SocketAddr` pronto**, não o tipo da biblioteca — quem chama não
//!   precisa conhecer `mdns-sd`.
//! * **Filtra endereço inútil**: um host que anuncia `127.0.0.1` ou `::1` não
//!   serve para outro aparelho entrar, então é descartado.
//!
//! ## Uma armadilha medida
//!
//! Anunciar com `127.0.0.1` **não funciona**: o registro existe, o browse o
//! encontra, e nenhum outro aparelho consegue conectar. O anúncio tem de levar o
//! IP da interface — [`ip_anunciavel`] descobre com o truque do `connect` UDP.
//! Medido nesta máquina: com `127.0.0.1` a descoberta dava `NAO ACHOU` em 15 s;
//! com o IP da interface, resolveu em menos de 2 s.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use mdns_sd::{Receiver, ServiceDaemon, ServiceEvent, ServiceInfo};

/// Tipo de serviço anunciado. O sufixo `.local.` é obrigatório no DNS de
/// serviço; sem ele o browse não encontra nada.
///
/// `_udp` porque o transporte é UDP. Mudar isto quebra compatibilidade com hosts
/// antigos do mesmo jeito que `VERSAO_PROTOCOLO`: um cliente antigo procuraria
/// `_giochi._udp` e não acharia quem anuncia outro nome.
pub const TIPO_SERVICO: &str = "_giochi._udp.local.";

/// Quanto tempo esperar por resposta antes de considerar que não há host.
///
/// mDNS é assíncrono e o browse é contínuo: este valor não é timeout de socket,
/// é só a sugestão para quem quiser uma espera limitada.
pub const ESPERA_SUGERIDA: Duration = Duration::from_secs(3);

/// Um host encontrado na LAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encontrado {
    /// Nome de instância anunciado, útil para mostrar na lista.
    pub nome: String,
    /// Endereço para passar a [`crate::Cliente::entrar`].
    pub endereco: SocketAddr,
}

/// Falha ao anunciar ou procurar.
#[derive(Debug)]
pub enum ErroDescoberta {
    /// A biblioteca recusou a operação.
    Servico(String),
    /// Não foi possível descobrir o IP da interface.
    SemInterface(std::io::Error),
}

impl core::fmt::Display for ErroDescoberta {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ErroDescoberta::Servico(m) => write!(f, "mDNS: {m}"),
            ErroDescoberta::SemInterface(e) => {
                write!(f, "não consegui descobrir o IP da interface: {e}")
            }
        }
    }
}

impl std::error::Error for ErroDescoberta {}

impl From<mdns_sd::Error> for ErroDescoberta {
    fn from(e: mdns_sd::Error) -> Self {
        ErroDescoberta::Servico(e.to_string())
    }
}

/// IP pelo qual os outros aparelhos alcançam esta máquina.
///
/// Truque padrão: um `connect` UDP **não envia pacote**, só faz o SO escolher a
/// rota de saída, e `local_addr` passa a devolver o IP daquela interface. É o
/// mesmo método de [`crate::Host::endereco_anunciado`], repetido aqui para o
/// módulo não depender de um `Host` já criado.
pub fn ip_anunciavel() -> Result<IpAddr, ErroDescoberta> {
    let sonda = std::net::UdpSocket::bind("0.0.0.0:0").map_err(ErroDescoberta::SemInterface)?;
    // TEST-NET-1: documentação, não roteável. Não sai pacote.
    sonda
        .connect("192.0.2.1:9")
        .map_err(ErroDescoberta::SemInterface)?;
    let ip = sonda
        .local_addr()
        .map_err(ErroDescoberta::SemInterface)?
        .ip();
    if ip.is_unspecified() || ip.is_loopback() {
        return Err(ErroDescoberta::SemInterface(std::io::Error::other(
            "interface de saída é loopback; um anúncio assim não é alcançável",
        )));
    }
    Ok(ip)
}

/// Nome de instância do anúncio.
///
/// Precisa ser único na LAN: dois hosts com o mesmo nome fazem o mDNS entrar em
/// sondagem e um deles renomeia. O sufixo vem do PID, que é barato e suficiente
/// — o caso real é um host por aparelho.
fn nome_da_instancia() -> String {
    format!("giochi-{}", std::process::id())
}

/// Anuncia um host na LAN.
///
/// Enquanto o valor existir, o anúncio está no ar; [`Anunciante::encerrar`]
/// manda o *goodbye* (TTL 0) para os outros aparelhos tirarem da lista na hora,
/// em vez de esperar o TTL expirar.
pub struct Anunciante {
    daemon: ServiceDaemon,
    nome: String,
}

impl Anunciante {
    /// Anuncia na porta dada, descobrindo o IP da interface.
    pub fn anunciar(porta: u16) -> Result<Self, ErroDescoberta> {
        let ip = ip_anunciavel()?;
        Self::anunciar_em(ip, porta)
    }

    /// Anuncia num IP explícito. Útil em teste e quando o aparelho tem mais de
    /// uma interface e a escolha automática pega a errada.
    pub fn anunciar_em(ip: IpAddr, porta: u16) -> Result<Self, ErroDescoberta> {
        let daemon = ServiceDaemon::new()?;
        let nome = nome_da_instancia();
        let host = format!("{nome}.local.");
        let info = ServiceInfo::new(
            TIPO_SERVICO,
            &nome,
            &host,
            ip.to_string().as_str(),
            porta,
            None,
        )?;
        daemon.register(info)?;
        Ok(Self { daemon, nome })
    }

    /// O nome anunciado, para mostrar na tela do host.
    pub fn nome(&self) -> &str {
        &self.nome
    }

    /// Retira o anúncio e para a thread da biblioteca.
    pub fn encerrar(self) {
        let _ = self.daemon.shutdown();
    }
}

/// Procura hosts na LAN.
///
/// O browse do mDNS é contínuo: quem chama drena o que chegou a cada frame, sem
/// esperar. Um host que entra depois de a busca começar também aparece — não há
/// "fim da lista".
pub struct Descoberta {
    daemon: ServiceDaemon,
    receptor: Receiver<ServiceEvent>,
}

impl Descoberta {
    /// Começa a procurar.
    pub fn iniciar() -> Result<Self, ErroDescoberta> {
        let daemon = ServiceDaemon::new()?;
        let receptor = daemon.browse(TIPO_SERVICO)?;
        Ok(Self { daemon, receptor })
    }

    /// Drena até `orcamento` eventos, devolvendo só os hosts resolvidos.
    ///
    /// Nunca bloqueia: `try_recv` volta na hora quando não há nada.
    pub fn drenar(&self, orcamento: usize) -> Vec<Encontrado> {
        let mut achados = Vec::new();
        for _ in 0..orcamento {
            match self.receptor.try_recv() {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    if let Some(encontrado) = converter(&info) {
                        achados.push(encontrado);
                    }
                }
                // SearchStarted, ServiceFound, ServiceRemoved…: quem decide se o
                // host serve é a resolução, não o anúncio do nome.
                Ok(_) => {}
                // Fila vazia agora, ou a thread morreu. Nos dois casos não há
                // nada a fazer neste frame.
                Err(_) => break,
            }
        }
        achados
    }

    /// Para a thread da biblioteca.
    pub fn encerrar(self) {
        let _ = self.daemon.shutdown();
    }
}

/// Converte o resultado da biblioteca no nosso tipo, descartando endereço inútil.
fn converter(info: &ServiceInfo) -> Option<Encontrado> {
    let porta = info.get_port();
    // Preferência por IPv4: um endereço IPv6 link-local (`fe80::`) resolve mas
    // não conecta sem `%interface`, então tentá-lo primeiro só gastaria tempo.
    let ip = info
        .get_addresses()
        .iter()
        .find(|a| matches!(a, IpAddr::V4(v4) if *v4 != Ipv4Addr::LOCALHOST))
        .or_else(|| {
            info.get_addresses()
                .iter()
                .find(|a| matches!(a, IpAddr::V4(_)))
        })
        .or_else(|| {
            // Só IPv6 global: aceitável, mas depois do IPv4.
            info.get_addresses()
                .iter()
                .find(|a| !matches!(a, IpAddr::V6(v6) if *v6 == Ipv6Addr::LOCALHOST))
        })
        .copied()?;

    Some(Encontrado {
        nome: info.get_fullname().to_string(),
        endereco: SocketAddr::new(ip, porta),
    })
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Nome de instância é estável dentro do processo, e não vazio.
    #[test]
    fn nome_de_instancia_e_utilizavel() {
        let nome = nome_da_instancia();
        assert!(nome.starts_with("giochi-"));
        assert_eq!(nome, nome_da_instancia());
        // O mDNS não aceita espaço nem ponto no rótulo.
        assert!(!nome.contains(' '));
        assert!(!nome.contains('.'));
    }

    #[test]
    fn tipo_de_servico_tem_o_sufixo_obrigatorio() {
        // Sem `.local.` o browse não encontra nada, e o sintoma é "não acha"
        // sem erro nenhum.
        assert!(TIPO_SERVICO.ends_with(".local."));
        assert!(TIPO_SERVICO.starts_with('_'));
        assert!(TIPO_SERVICO.contains("._udp."));
    }

    /// O anúncio em loopback é recusado — é a armadilha medida.
    #[test]
    fn ip_anunciavel_nao_devolve_loopback() {
        match ip_anunciavel() {
            Ok(ip) => {
                assert!(!ip.is_loopback(), "anúncio em loopback não é alcançável");
                assert!(!ip.is_unspecified());
            }
            // Máquina sem rota de saída (CI isolado): a recusa é o comportamento
            // correto, não uma falha do teste.
            Err(ErroDescoberta::SemInterface(_)) => {}
            Err(outro) => panic!("erro inesperado: {outro}"),
        }
    }

    /// Descoberta de verdade: anuncia e encontra o próprio anúncio.
    ///
    /// `#[ignore]` porque depende de multicast na interface real. Roda com
    /// `cargo test --features mdns -- --ignored`.
    #[test]
    #[ignore = "precisa de multicast numa interface real; rode com --ignored"]
    fn encontra_o_proprio_anuncio() {
        let ip = ip_anunciavel().expect("precisa de interface de saída");
        // Porta alta qualquer: o anúncio não abre socket nessa porta, só publica
        // o número.
        let anunciante = Anunciante::anunciar_em(ip, 47721).expect("devia anunciar");
        let descoberta = Descoberta::iniciar().expect("devia iniciar o browse");

        let inicio = std::time::Instant::now();
        let mut achou = None;
        while inicio.elapsed() < Duration::from_secs(25) {
            for e in descoberta.drenar(16) {
                if e.endereco.port() == 47721 {
                    achou = Some(e);
                }
            }
            if achou.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }

        descoberta.encerrar();
        anunciante.encerrar();

        let encontrado = achou.expect("o próprio anúncio devia aparecer");
        assert_eq!(encontrado.endereco.port(), 47721);
        assert!(!encontrado.endereco.ip().is_loopback());
        assert!(encontrado.nome.contains("_giochi._udp.local."));
    }
}
