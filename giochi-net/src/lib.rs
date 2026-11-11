//! `giochi-net` — camada de rede P2P do GIOCHI-ENGINE-3D.
//!
//! Especificação: `Relatório consultor externo.md`, relatório 2 (§Arquitetura,
//! §Protocolo, §Interest Management).
//!
//! ## Princípios que este crate segue, e por quê
//!
//! 1. **Síncrono, sem `tokio`.** O laço da engine é de timestep fixo a 60 Hz e
//!    roda num Cortex-A55/A35 com 1–2 GB. Um runtime assíncrono multi-thread
//!    traria thread pool e alocador próprios para fazer I/O de alguns KB/s.
//!    `std::net::UdpSocket` em modo **não-bloqueante** faz o mesmo sem runtime.
//!
//! 2. **`aarch64-unknown-linux-gnu` sem dependência de sistema.** Nenhum crate
//!    C, nenhum `lib` além do libc. É o requisito do relatório e a razão de a
//!    serialização ser feita à mão (ver [`wire`]).
//!
//! 3. **Nunca bloquear o timestep.** Toda função pública de I/O devolve
//!    `WouldBlock` como resultado normal, não como erro. Quem chama drena o que
//!    houver e segue; se a rede parar, o jogo continua.
//!
//! 4. **A engine não conhece regra de jogo.** O crate transporta bytes com
//!    estrutura. "Onda", "chefe", "torre" e "loja" são do jogo — a engine só
//!    garante que o outro lado receba o que ele mandou, na ordem em que a
//!    aplicação pediu.
//!
//! ## O que este crate NÃO faz
//!
//! - NAT traversal / relay. LAN apenas, como o relatório declara no "fora de
//!   escopo".
//! - Descoberta por mDNS **ainda**: depende de `mdns-sd`, que puxa `if-addrs` e
//!   `socket2`. Ver `docs/REDE.md` para o estado e a decisão.
//! - Reconexão automática. Handshake é explícito; quem chama decide reenviar.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod aoi;
pub mod client;
pub mod host;
pub mod peer;
pub mod wire;

pub use aoi::{filtrar, AreaDeInteresse, FiltroAoi};
pub use client::{Cliente, Estado};
pub use host::{ClienteId, Host, MAX_CLIENTES, TIMEOUT_CLIENTE};
pub use peer::{Evento, Modo, Net};
pub use wire::{ErroLeitura, Mensagem, ResumoJogador, SnapshotInimigo};

/// Versão do protocolo. Cliente e host com versões diferentes **não conversam**,
/// e o handshake recusa antes de qualquer estado ser trocado.
///
/// Mudar o layout de um `enum` em [`wire`] exige subir isto. Um cliente antigo
/// falando com host novo produziria leitura deslocada — pior que recusar.
pub const VERSAO_PROTOCOLO: u16 = 1;

/// Tamanho máximo de um datagrama que este crate aceita.
///
/// 1200 bytes: menor que o MTU mínimo de IPv6 (1280) menos cabeçalhos, então um
/// snapshot cabe num datagrama sem fragmentação em qualquer rede onde o alvo é
/// alcançável. Fragmentar UDP é a forma mais barata de perder pacote inteiro por
/// causa de uma perda de 20 %.
pub const DATAGRAMA_MAX: usize = 1200;

/// Taxa de snapshot recomendada, em Hz.
///
/// O relatório pede 10–20 Hz enquanto o render é 60 Hz. 15 Hz é o meio: com 150
/// inimigos na AOI dá poucos KB/s por cliente, e a interpolação no cliente cobre
/// o intervalo sem parecer travado.
pub const SNAPSHOT_HZ: u32 = 15;

/// Porta padrão. Escolhida na faixa efêmera-alta, longe de 22 (SSH do USB) e
/// 8080/8088 (painel e loja), para não colidir num aparelho já ocupado.
pub const PORTA_PADRAO: u16 = 47721;

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn versao_e_porta_sao_estaveis() {
        // Se um destes mudar sem intenção, clientes e hosts deixam de se falar
        // e o sintoma aparece só em campo. Travar aqui obriga a mudança a ser
        // consciente.
        assert_eq!(VERSAO_PROTOCOLO, 1);
        assert_eq!(PORTA_PADRAO, 47721);
        assert_eq!(SNAPSHOT_HZ, 15);
    }

    #[test]
    fn datagrama_cabe_no_mtu_minimo_de_ipv6() {
        // 1280 é o MTU mínimo que todo host IPv6 é obrigado a suportar; tirando
        // 40 (IPv6) + 8 (UDP) sobram 1232.
        assert!(DATAGRAMA_MAX <= 1232, "datagrama fragmentaria em rede padrão");
    }
}
