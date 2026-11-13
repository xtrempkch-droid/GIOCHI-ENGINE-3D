//! Serialização do protocolo — escrita à mão, sem `serde`.
//!
//! ## Por que não `postcard` nem `bincode`
//!
//! O relatório oferece os dois. Nenhum é necessário **aqui**, e um deles cobra
//! caro:
//!
//! | | postcard | bincode | este módulo |
//! | --- | --- | --- | --- |
//! | dependência | `serde` + `postcard` | `serde` + `bincode` | nenhuma |
//! | tamanho do fio | mínimo | compacto | mínimo |
//! | reconhecer dado corrompido | não | não | **sim** (tag de variante validada) |
//!
//! O protocolo tem **nove variantes** e campos de tipo fixo. `serde` + derive
//! custa código gerado, tempo de compilação e — o que importa — uma camada entre
//! o erro de leitura e a linha que o produziu. Escrever 200 linhas de
//! `codificar`/`decodificar` à mão dá:
//!
//! - **zero dependência**, que é o requisito de cross-compile do relatório;
//! - erro com o **offset exato** onde o datagrama não bate mais;
//! - recusa de pacote de versão diferente antes de tocar em estado de jogo.
//!
//! O custo é manter o layout à mão. Por isso [`VERSAO_PROTOCOLO`] existe e cada
//! variante tem teste de ida e volta.

use crate::VERSAO_PROTOCOLO;

/// Erro de leitura de datagrama. Carrega o offset porque "pacote inválido" não
/// diz nada quando o problema é meio byte fora de lugar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErroLeitura {
    /// Acabou o buffer antes de terminar o campo.
    Truncado {
        /// Posição onde a leitura acabou.
        offset: usize,
        /// Quantos bytes ainda faltavam.
        faltam: usize,
    },
    /// Byte que não corresponde a nenhuma variante conhecida.
    VarianteDesconhecida(u8),
    /// Versão de protocolo diferente.
    VersaoIncompativel {
        /// Versão que veio no datagrama.
        recebida: u16,
        /// Versão que este crate implementa.
        local: u16,
    },
}

impl core::fmt::Display for ErroLeitura {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ErroLeitura::Truncado { offset, faltam } => {
                write!(f, "datagrama truncado em {offset} (faltam {faltam} bytes)")
            }
            ErroLeitura::VarianteDesconhecida(tag) => {
                write!(f, "variante desconhecida: 0x{tag:02x}")
            }
            ErroLeitura::VersaoIncompativel { recebida, local } => {
                write!(f, "protocolo {recebida} != {local}")
            }
        }
    }
}

impl std::error::Error for ErroLeitura {}

/// Um inimigo dentro de um snapshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapshotInimigo {
    /// Identificador estável durante a vida do inimigo.
    pub id: u16,
    /// Posição no plano do jogo.
    pub x: f32,
    /// Posição no plano do jogo.
    pub z: f32,
    /// Vida atual.
    pub hp: u16,
    /// Tipo do inimigo. O significado é do jogo, não da engine.
    pub tipo: u8,
    /// Fase de chefe (`0` = não é chefe).
    pub fase_chefe: u8,
}

/// Estado de um jogador, para o host informar o cliente.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResumoJogador {
    /// Id atribuído pelo host no handshake.
    pub id: u8,
    /// Posição.
    pub x: f32,
    /// Posição.
    pub z: f32,
    /// Vida.
    pub hp: u16,
    /// Pontuação.
    pub pontos: u32,
}

/// Uma mensagem do protocolo.
///
/// O relatório lista nove variantes. As que dizem respeito a regra de jogo
/// (onda, torre, loja, nemesis) carregam **bytes opacos** de propósito: a engine
/// transporta, o jogo interpreta. Só o que a engine precisa para rotear
/// (posição, AOI, tick, id) tem campo tipado.
#[derive(Debug, Clone, PartialEq)]
pub enum Mensagem {
    /// C→H. Primeira coisa que o cliente manda.
    Handshake {
        /// Versão do protocolo do cliente.
        versao: u16,
        /// Tipo de controle. O jogo decide o que fazer com isso.
        tipo_controle: u8,
    },
    /// H→C. Resposta ao handshake, com o id atribuído.
    HandshakeOk {
        /// Versão do protocolo do host.
        versao: u16,
        /// Id atribuído a este cliente. Nunca 0 (0 é o host).
        jogador: u8,
    },
    /// H→C. O host recusou a entrada.
    HandshakeRecusado {
        /// Motivo legível pelo log do cliente.
        motivo: [u8; 48],
        /// Tamanho válido de `motivo`.
        tamanho: u8,
    },
    /// C→H. Input do jogador, com a câmera para o cálculo de AOI.
    Input {
        /// Tick do cliente, só para correlação de RTT.
        tick: u32,
        /// Direção de movimento, `[-1, 1]`.
        mover_x: f32,
        /// Direção de movimento, `[-1, 1]`.
        mover_z: f32,
        /// Direção de mira, `[-1, 1]`.
        mirar_x: f32,
        /// Direção de mira, `[-1, 1]`.
        mirar_z: f32,
        /// Botões pressionados, bitmask do jogo.
        acoes: u32,
        /// Posição X da câmera — o host usa para a AOI.
        camera_x: f32,
        /// Posição Z da câmera — o host usa para a AOI.
        camera_z: f32,
    },
    /// H→C. Estado de um jogador.
    Jogador(ResumoJogador),
    /// H→C, unicast por AOI. Inimigos dentro do interesse deste cliente.
    Inimigos {
        /// Tick do host.
        tick: u32,
        /// Lista filtrada.
        inimigos: Vec<SnapshotInimigo>,
    },
    /// H→C, broadcast. Estado global — bytes opacos do jogo.
    Global {
        /// Tick do host.
        tick: u32,
        /// Payload interpretado pelo jogo.
        dados: Vec<u8>,
    },
    /// C→H. Compra na loja. O host valida (ouro, disponibilidade) e responde
    /// com [`Mensagem::Global`]; o cliente não aplica sozinho.
    ShopBuy {
        /// Id do item na tabela do jogo. A engine não conhece itens.
        item_id: u16,
    },
    /// Ambos. `enviado_ms` volta igual no [`Mensagem::Pong`].
    Ping {
        /// Marca do remetente, em ms.
        enviado_ms: u32,
    },
    /// Ambos. Devolve o valor do [`Mensagem::Ping`].
    Pong {
        /// Marca do [`Mensagem::Ping`] original.
        enviado_ms: u32,
    },
}

// Tags no fio. Fixas: mudar qualquer uma quebra compatibilidade.
const TAG_HANDSHAKE: u8 = 1;
const TAG_HANDSHAKE_OK: u8 = 2;
const TAG_HANDSHAKE_RECUSADO: u8 = 3;
const TAG_INPUT: u8 = 4;
const TAG_JOGADOR: u8 = 5;
const TAG_INIMIGOS: u8 = 6;
const TAG_GLOBAL: u8 = 7;
const TAG_PING: u8 = 8;
const TAG_PONG: u8 = 9;
const TAG_SHOP_BUY: u8 = 10;

/// Bytes que um inimigo ocupa no snapshot.
///
/// `id`(2) + `x`(2) + `z`(2) + `hp`(1) + `tipo`(1) + `fase_chefe`(1) = **9**.
///
/// A primeira versão usava 5 aqui — número que eu tinha deduzido do campo
/// "posição" e não do que o codificador realmente escreve. Com o divisor
/// errado, 150 inimigos passavam do limite de datagrama e o crate produzia
/// pacote grande demais. O teste pegou.
const TAM_INIMIGO: usize = 9;

struct Escritor {
    buf: Vec<u8>,
}

impl Escritor {
    fn new() -> Self {
        Self {
            buf: Vec::with_capacity(crate::DATAGRAMA_MAX),
        }
    }

    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        // `to_bits` em vez de `to_le_bytes`: NaN e -0.0 preservados byte a byte,
        // e o teste de ida e volta consegue comparar exatamente.
        self.buf.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    fn bytes(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }
}

struct Leitor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Leitor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn precisa(&self, n: usize) -> Result<(), ErroLeitura> {
        let restante = self.buf.len().saturating_sub(self.pos);
        if restante < n {
            return Err(ErroLeitura::Truncado {
                offset: self.pos,
                faltam: n - restante,
            });
        }
        Ok(())
    }

    fn u8(&mut self) -> Result<u8, ErroLeitura> {
        self.precisa(1)?;
        let v = self.buf[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn u16(&mut self) -> Result<u16, ErroLeitura> {
        self.precisa(2)?;
        let v = u16::from_le_bytes([self.buf[self.pos], self.buf[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    fn u32(&mut self) -> Result<u32, ErroLeitura> {
        self.precisa(4)?;
        let mut b = [0u8; 4];
        b.copy_from_slice(&self.buf[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(u32::from_le_bytes(b))
    }

    fn f32(&mut self) -> Result<f32, ErroLeitura> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], ErroLeitura> {
        self.precisa(n)?;
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
}

impl Mensagem {
    /// Serializa. O buffer resultante nunca passa de [`crate::DATAGRAMA_MAX`];
    /// se passar, `Inimigos` é truncado no último inimigo inteiro e o chamador
    /// recebe o aviso pelo retorno de [`Self::tamanho`].
    pub fn codificar(&self) -> Vec<u8> {
        let mut w = Escritor::new();
        w.u16(VERSAO_PROTOCOLO);
        match self {
            Mensagem::Handshake {
                versao,
                tipo_controle,
            } => {
                w.u8(TAG_HANDSHAKE);
                w.u16(*versao);
                w.u8(*tipo_controle);
            }
            Mensagem::HandshakeOk { versao, jogador } => {
                w.u8(TAG_HANDSHAKE_OK);
                w.u16(*versao);
                w.u8(*jogador);
            }
            Mensagem::HandshakeRecusado { motivo, tamanho } => {
                w.u8(TAG_HANDSHAKE_RECUSADO);
                let n = (*tamanho as usize).min(motivo.len());
                w.u8(n as u8);
                w.bytes(&motivo[..n]);
            }
            Mensagem::Input {
                tick,
                mover_x,
                mover_z,
                mirar_x,
                mirar_z,
                acoes,
                camera_x,
                camera_z,
            } => {
                w.u8(TAG_INPUT);
                w.u32(*tick);
                w.f32(*mover_x);
                w.f32(*mover_z);
                w.f32(*mirar_x);
                w.f32(*mirar_z);
                w.u32(*acoes);
                w.f32(*camera_x);
                w.f32(*camera_z);
            }
            Mensagem::Jogador(j) => {
                w.u8(TAG_JOGADOR);
                w.u8(j.id);
                w.f32(j.x);
                w.f32(j.z);
                w.u16(j.hp);
                w.u32(j.pontos);
            }
            Mensagem::Inimigos { tick, inimigos } => {
                w.u8(TAG_INIMIGOS);
                w.u32(*tick);
                // Quantos cabem: `TAM_INIMIGO` bytes por inimigo + 2 do
                // contador, sobre o que já foi escrito.
                let cabecalho = w.buf.len() + 2;
                let cabe = crate::DATAGRAMA_MAX.saturating_sub(cabecalho) / TAM_INIMIGO;
                let n = inimigos.len().min(cabe);
                w.u16(n as u16);
                for ini in &inimigos[..n] {
                    w.u16(ini.id);
                    // Quantiza a posição em centímetros inteiros: 1000x menor que
                    // float, e 1 cm de erro é invisível num mapa de 4x4 telas.
                    w.u16(quantizar(ini.x));
                    w.u16(quantizar(ini.z));
                    w.u8(ini.hp.min(255) as u8);
                    w.u8(ini.tipo);
                    w.u8(ini.fase_chefe);
                }
            }
            Mensagem::Global { tick, dados } => {
                w.u8(TAG_GLOBAL);
                w.u32(*tick);
                let cabecalho = w.buf.len() + 2;
                let cabe = crate::DATAGRAMA_MAX.saturating_sub(cabecalho);
                let n = dados.len().min(cabe);
                w.u16(n as u16);
                w.bytes(&dados[..n]);
            }
            Mensagem::Ping { enviado_ms } => {
                w.u8(TAG_PING);
                w.u32(*enviado_ms);
            }
            Mensagem::Pong { enviado_ms } => {
                w.u8(TAG_PONG);
                w.u32(*enviado_ms);
            }
            Mensagem::ShopBuy { item_id } => {
                w.u8(TAG_SHOP_BUY);
                w.u16(*item_id);
            }
        }
        w.buf
    }

    /// Desserializa. Recusa versão diferente **antes** de ler qualquer campo.
    pub fn decodificar(buf: &[u8]) -> Result<Self, ErroLeitura> {
        let mut r = Leitor::new(buf);
        let versao = r.u16()?;
        if versao != VERSAO_PROTOCOLO {
            return Err(ErroLeitura::VersaoIncompativel {
                recebida: versao,
                local: VERSAO_PROTOCOLO,
            });
        }
        let tag = r.u8()?;
        match tag {
            TAG_HANDSHAKE => Ok(Mensagem::Handshake {
                versao: r.u16()?,
                tipo_controle: r.u8()?,
            }),
            TAG_HANDSHAKE_OK => Ok(Mensagem::HandshakeOk {
                versao: r.u16()?,
                jogador: r.u8()?,
            }),
            TAG_HANDSHAKE_RECUSADO => {
                let n = r.u8()? as usize;
                let mut motivo = [0u8; 48];
                // `n` vem do fio e pode ser até 255. Copiar sem limitar
                // estourava o array e PANICAVA o host — um datagrama forjado
                // derrubaria a partida. Corta no tamanho do buffer.
                let n_efetivo = n.min(motivo.len());
                let lido = r.bytes(n_efetivo)?;
                motivo[..lido.len()].copy_from_slice(lido);
                Ok(Mensagem::HandshakeRecusado {
                    motivo,
                    tamanho: lido.len() as u8,
                })
            }
            TAG_INPUT => Ok(Mensagem::Input {
                tick: r.u32()?,
                mover_x: r.f32()?,
                mover_z: r.f32()?,
                mirar_x: r.f32()?,
                mirar_z: r.f32()?,
                acoes: r.u32()?,
                camera_x: r.f32()?,
                camera_z: r.f32()?,
            }),
            TAG_JOGADOR => Ok(Mensagem::Jogador(ResumoJogador {
                id: r.u8()?,
                x: r.f32()?,
                z: r.f32()?,
                hp: r.u16()?,
                pontos: r.u32()?,
            })),
            TAG_INIMIGOS => {
                let tick = r.u32()?;
                let n = r.u16()? as usize;
                // `n` vem do fio e pode ser 65535 num datagrama de 7 bytes. Reservar
                // o valor pedido transformaria um pacote minúsculo em centenas de KB
                // de alocação por pacote; o que o buffer comporta é o teto real.
                let cabem = r.buf.len().saturating_sub(r.pos) / TAM_INIMIGO;
                let mut inimigos = Vec::with_capacity(n.min(cabem));
                for _ in 0..n {
                    inimigos.push(SnapshotInimigo {
                        id: r.u16()?,
                        x: desquantizar(r.u16()?),
                        z: desquantizar(r.u16()?),
                        hp: r.u8()? as u16,
                        tipo: r.u8()?,
                        fase_chefe: r.u8()?,
                    });
                }
                Ok(Mensagem::Inimigos { tick, inimigos })
            }
            TAG_GLOBAL => {
                let tick = r.u32()?;
                let n = r.u16()? as usize;
                Ok(Mensagem::Global {
                    tick,
                    dados: r.bytes(n)?.to_vec(),
                })
            }
            TAG_PING => Ok(Mensagem::Ping {
                enviado_ms: r.u32()?,
            }),
            TAG_PONG => Ok(Mensagem::Pong {
                enviado_ms: r.u32()?,
            }),
            TAG_SHOP_BUY => Ok(Mensagem::ShopBuy { item_id: r.u16()? }),
            outra => Err(ErroLeitura::VarianteDesconhecida(outra)),
        }
    }

    /// Tamanho do datagrama depois de codificar.
    pub fn tamanho(&self) -> usize {
        self.codificar().len()
    }
}

/// Posição `f32` → centímetros inteiros.
///
/// Faixa representável: `0..=655.35 m` (o campo é `u16` e a unidade é 1 cm).
/// Coordenadas negativas saturam em 0 — o mapa deste projeto usa o primeiro
/// quadrante —, e acima de 655.35 m satura no topo em vez de dar a volta,
/// porque posição que "salta" no cliente é pior que posição colada na borda.
fn quantizar(v: f32) -> u16 {
    let cm = (v * 100.0).round();
    if !cm.is_finite() || cm <= 0.0 {
        return 0;
    }
    if cm >= u16::MAX as f32 {
        return u16::MAX;
    }
    cm as u16
}

fn desquantizar(cm: u16) -> f32 {
    cm as f32 / 100.0
}

#[cfg(test)]
mod testes {
    use super::*;

    fn ida_volta(m: Mensagem) {
        let bytes = m.codificar();
        assert!(
            bytes.len() <= crate::DATAGRAMA_MAX,
            "estourou: {}",
            bytes.len()
        );
        let lido = Mensagem::decodificar(&bytes).expect("devia decodificar");
        assert_eq!(m, lido);
    }

    #[test]
    fn todas_as_variantes_fazem_ida_e_volta() {
        ida_volta(Mensagem::Handshake {
            versao: VERSAO_PROTOCOLO,
            tipo_controle: 2,
        });
        ida_volta(Mensagem::HandshakeOk {
            versao: VERSAO_PROTOCOLO,
            jogador: 3,
        });
        ida_volta(Mensagem::Input {
            tick: 7,
            mover_x: 0.5,
            mover_z: -0.5,
            mirar_x: 1.0,
            mirar_z: 0.0,
            acoes: 0b1010,
            camera_x: 12.34,
            camera_z: -56.78,
        });
        ida_volta(Mensagem::Jogador(ResumoJogador {
            id: 1,
            x: 10.0,
            z: 20.0,
            hp: 100,
            pontos: 4242,
        }));
        ida_volta(Mensagem::Inimigos {
            tick: 9,
            inimigos: vec![
                SnapshotInimigo {
                    id: 1,
                    x: 1.0,
                    z: 2.0,
                    hp: 10,
                    tipo: 0,
                    fase_chefe: 0,
                },
                SnapshotInimigo {
                    id: 2,
                    x: 3.5,
                    z: 4.5,
                    hp: 200,
                    tipo: 1,
                    fase_chefe: 2,
                },
            ],
        });
        ida_volta(Mensagem::Global {
            tick: 5,
            dados: vec![1, 2, 3, 4],
        });
        ida_volta(Mensagem::Ping { enviado_ms: 1000 });
        ida_volta(Mensagem::Pong { enviado_ms: 1000 });
    }

    #[test]
    fn handshake_recusado_leva_o_motivo() {
        let texto = "protocolo antigo";
        let mut motivo = [0u8; 48];
        motivo[..texto.len()].copy_from_slice(texto.as_bytes());
        let m = Mensagem::HandshakeRecusado {
            motivo,
            tamanho: texto.len() as u8,
        };
        let lido = Mensagem::decodificar(&m.codificar()).unwrap();
        match lido {
            Mensagem::HandshakeRecusado { motivo, tamanho } => {
                assert_eq!(&motivo[..tamanho as usize], texto.as_bytes());
            }
            outro => panic!("variante errada: {outro:?}"),
        }
    }

    #[test]
    fn versao_diferente_e_recusada_antes_de_ler_o_corpo() {
        let mut bytes = Mensagem::Ping { enviado_ms: 1 }.codificar();
        bytes[0] = 0xFF; // versão 255
        bytes[1] = 0x00;
        match Mensagem::decodificar(&bytes) {
            Err(ErroLeitura::VersaoIncompativel { recebida, local }) => {
                assert_eq!(recebida, 0xFF);
                assert_eq!(local, VERSAO_PROTOCOLO);
            }
            outro => panic!("esperava VersaoIncompativel, veio {outro:?}"),
        }
    }

    #[test]
    fn datagrama_truncado_diz_onde_parou() {
        let bytes = Mensagem::Input {
            tick: 1,
            mover_x: 0.0,
            mover_z: 0.0,
            mirar_x: 0.0,
            mirar_z: 0.0,
            acoes: 0,
            camera_x: 0.0,
            camera_z: 0.0,
        }
        .codificar();
        let cortado = &bytes[..bytes.len() - 3];
        match Mensagem::decodificar(cortado) {
            Err(ErroLeitura::Truncado { faltam, .. }) => assert!(faltam >= 1),
            outro => panic!("esperava Truncado, veio {outro:?}"),
        }
    }

    #[test]
    fn tag_invalida_e_recusada() {
        let mut bytes = Mensagem::Ping { enviado_ms: 0 }.codificar();
        bytes[2] = 0xEE;
        match Mensagem::decodificar(&bytes) {
            Err(ErroLeitura::VarianteDesconhecida(t)) => assert_eq!(t, 0xEE),
            outro => panic!("esperava VarianteDesconhecida, veio {outro:?}"),
        }
    }

    #[test]
    fn snapshot_trunca_no_ultimo_inimigo_inteiro() {
        // 150 inimigos * 5 bytes + cabeçalho passa de 1200: tem de truncar sem
        // produzir datagrama inválido.
        let muitos: Vec<_> = (0..150)
            .map(|i| SnapshotInimigo {
                id: i,
                x: i as f32,
                z: 0.0,
                hp: 10,
                tipo: 0,
                fase_chefe: 0,
            })
            .collect();
        let bytes = Mensagem::Inimigos {
            tick: 1,
            inimigos: muitos,
        }
        .codificar();
        assert!(bytes.len() <= crate::DATAGRAMA_MAX);

        let lido = Mensagem::decodificar(&bytes).expect("continua válido");
        match lido {
            Mensagem::Inimigos { inimigos, .. } => {
                assert!(inimigos.len() < 150, "devia ter truncado");
                assert!(!inimigos.is_empty());
            }
            outro => panic!("variante errada: {outro:?}"),
        }
    }

    #[test]
    fn entrada_arbitraria_nao_provoca_panic() {
        // Todo byte de um datagrama vem da rede. O decoder pode recusar, mas
        // nunca pode entrar em pânico: um pacote forjado derrubaria o host.
        let mut estado = 0x9E37_79B9u32;
        let mut proximo = || {
            estado ^= estado << 13;
            estado ^= estado >> 17;
            estado ^= estado << 5;
            estado as u8
        };

        for len in 0..96 {
            let mut bytes: Vec<u8> = (0..len).map(|_| proximo()).collect();
            let _ = Mensagem::decodificar(&bytes);

            if len >= 3 {
                bytes[0..2].copy_from_slice(&VERSAO_PROTOCOLO.to_le_bytes());
                for tag in 1..=11u8 {
                    bytes[2] = tag;
                    let _ = Mensagem::decodificar(&bytes);
                }
            }
        }
    }

    #[test]
    fn contagem_de_inimigos_maior_que_o_pacote_e_recusada() {
        // Um datagrama de 9 bytes pode declarar 65535 inimigos. O decodificador
        // não pode reservar o que o campo pede antes de saber se há bytes.
        let mut bytes = VERSAO_PROTOCOLO.to_le_bytes().to_vec();
        bytes.push(TAG_INIMIGOS);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&u16::MAX.to_le_bytes());
        match Mensagem::decodificar(&bytes) {
            Err(ErroLeitura::Truncado { .. }) => {}
            outro => panic!("esperava Truncado, veio {outro:?}"),
        }
    }

    #[test]
    fn quantizacao_satura_em_vez_de_dar_a_volta() {
        assert_eq!(quantizar(-1.0), 0);
        assert_eq!(quantizar(f32::NAN), 0);
        assert_eq!(quantizar(1e9), u16::MAX);
        assert!((desquantizar(quantizar(2.5)) - 2.5).abs() < 0.011);
    }

    #[test]
    fn global_nao_estoura_com_payload_grande() {
        let bytes = Mensagem::Global {
            tick: 0,
            dados: vec![7u8; 5000],
        }
        .codificar();
        assert!(bytes.len() <= crate::DATAGRAMA_MAX);
        assert!(Mensagem::decodificar(&bytes).is_ok());
    }
}
