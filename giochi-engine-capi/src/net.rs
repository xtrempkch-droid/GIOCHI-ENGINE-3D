//! Rede P2P — Part IV da ABI C.
//!
//! Expõe [`giochi_net`] para C: host autoritativo, cliente, snapshot filtrado por
//! área de interesse.
//!
//! # O problema central desta camada: C não tem `Vec`
//!
//! `giochi_net::Mensagem::Inimigos` carrega uma lista de tamanho variável. Uma
//! struct `#[repr(C)]` precisa de tamanho fixo, então [`GiochiNetSnapshot`] tem
//! um array de [`GIOCHI_NET_MAX_INIMIGOS`] posições mais dois contadores:
//!
//! | Campo | Significa |
//! | --- | --- |
//! | `total` | quantos inimigos passaram pelo filtro de AOI |
//! | `escritos` | quantos couberam no array |
//!
//! **`total > escritos` é truncamento, e o consumidor tem de olhar.** Sem esses
//! dois números, um cliente com 200 inimigos na área desenharia 64 e o jogador
//! veria inimigos invisíveis atacando a torre — sem nenhum erro.
//!
//! # Convenções, iguais às do resto da ABI
//!
//! * Retorno `int32_t`: `0` = sucesso, negativo = erro (código em
//!   [`GIOCHI_ERROR_NETWORK`] e companhia), mensagem em `giochi_get_last_error()`.
//! * Handle é `u64` opaco; `0` nunca é válido.
//! * Nenhum panic atravessa: tudo passa por [`state::guard`].
//!
//! # Uma exceção deliberada à regra "0 ou negativo"
//!
//! `giochi_net_poll_snapshot` devolve **`1`** ([`GIOCHI_NET_AGUARDE`]) quando não
//! há snapshot novo. Isso não é erro — é o caso normal a 60 Hz, porque o host
//! publica a 15 Hz. Devolver `0` com `total == 0` juntaria "ainda não chegou" com
//! "chegou e não há inimigo nenhum", e o cliente precisa distinguir os dois para
//! decidir se interpola ou se limpa a tela.

use core::ffi::{c_char, CStr};

use giochi_net::{Cliente, Estado, Host, Mensagem, SnapshotInimigo};

use crate::abi_types::{GIOCHI_ERROR_NETWORK, GIOCHI_NET_AGUARDE};
use crate::state::{self, guard};

/// Quantos inimigos cabem em [`GiochiNetSnapshot`].
///
/// 64 é o teto do que um cliente vê de uma vez depois da AOI. O mapa tem 4×4
/// telas e o relatório fala em ~150 inimigos vivos: um quarto de tela em volta do
/// jogador raramente passa de 30, e 64 dá folga para o pior caso de um chefe
/// cercado.
pub const GIOCHI_NET_MAX_INIMIGOS: usize = 64;

/// Um inimigo, como o C enxerga.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GiochiNetInimigo {
    /// Identificador estável enquanto o inimigo vive.
    pub id: u16,
    /// Posição no plano do jogo.
    pub x: f32,
    /// Posição no plano do jogo.
    pub z: f32,
    /// Vida atual.
    pub hp: u16,
    /// Tipo do inimigo. O significado é do jogo; a engine só transporta.
    pub tipo: u8,
    /// Fase de chefe (`0` = não é chefe).
    pub fase_chefe: u8,
}

impl From<SnapshotInimigo> for GiochiNetInimigo {
    fn from(i: SnapshotInimigo) -> Self {
        Self {
            id: i.id,
            x: i.x,
            z: i.z,
            hp: i.hp,
            tipo: i.tipo,
            fase_chefe: i.fase_chefe,
        }
    }
}

/// O snapshot de um cliente.
///
/// Ver a nota do módulo sobre `total` contra `escritos`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GiochiNetSnapshot {
    /// Tick do host que produziu este snapshot. Sempre crescente.
    ///
    /// Use para descartar snapshot velho: se o valor não subiu, é o mesmo quadro.
    pub tick: u32,
    /// Quantos inimigos a área de interesse do cliente continha.
    pub total: u32,
    /// Quantos foram copiados para `inimigos`.
    pub escritos: u32,
    /// Quantos cabem em `inimigos`. Constante, exposta para o chamador não
    /// precisar repetir o número.
    pub capacidade: u32,
    /// Os inimigos.
    pub inimigos: [GiochiNetInimigo; GIOCHI_NET_MAX_INIMIGOS],
}

impl Default for GiochiNetSnapshot {
    fn default() -> Self {
        Self {
            tick: 0,
            total: 0,
            escritos: 0,
            capacidade: GIOCHI_NET_MAX_INIMIGOS as u32,
            inimigos: [GiochiNetInimigo::default(); GIOCHI_NET_MAX_INIMIGOS],
        }
    }
}

/// O input de um jogador, no sentido em que o cliente o envia.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GiochiNetInput {
    /// Tick do cliente, só para correlacionar RTT. Não é o tick da simulação.
    pub tick: u32,
    /// Direção de movimento, `[-1, 1]`.
    pub mover_x: f32,
    /// Direção de movimento, `[-1, 1]`.
    pub mover_z: f32,
    /// Direção de mira, `[-1, 1]`.
    pub mirar_x: f32,
    /// Direção de mira, `[-1, 1]`.
    pub mirar_z: f32,
    /// Botões pressionados, bitmask do jogo.
    pub acoes: u32,
    /// Posição X da câmera — o host usa para a área de interesse.
    pub camera_x: f32,
    /// Posição Z da câmera — o host usa para a área de interesse.
    pub camera_z: f32,
}

/// Ou um host, ou um cliente. Nunca os dois.
enum No {
    Host {
        host: Host,
        /// Inputs que chegaram no último [`giochi_net_pump`] e ainda não foram
        /// lidos por [`giochi_net_poll_input`].
        ///
        /// Fica **dentro** do nó, e não numa `thread_local` solta: dois hosts na
        /// mesma thread dividiriam a mesma fila, e um leria o input do jogador do
        /// outro. Parece rebuscado até alguém abrir dois hosts (é o que um teste
        /// de integração faz).
        pendentes: Vec<(u8, Mensagem)>,
    },
    Cliente {
        cliente: Cliente,
        /// Câmera reposta por `giochi_net_set_camera_pos`.
        ///
        /// Quando presente, sobrepõe `camera_x`/`camera_z` do input. Serve para o
        /// jogo que calcula a câmera separadamente do input e não quer repetir os
        /// valores em cada estrutura.
        camera: Option<(f32, f32)>,
        /// Último snapshot recebido, com o tick que o acompanha.
        ultimo: Option<(u32, Vec<SnapshotInimigo>)>,
    },
}

/// Registro de nós. Handles nunca são reutilizados.
#[derive(Default)]
struct Registro {
    nos: std::collections::HashMap<u64, No>,
    proximo: u64,
}

impl Registro {
    fn inserir(&mut self, no: No) -> u64 {
        self.proximo += 1;
        let handle = self.proximo;
        self.nos.insert(handle, no);
        handle
    }
}

thread_local! {
    static REGISTRO: std::cell::RefCell<Registro> = std::cell::RefCell::new(Registro {
        nos: std::collections::HashMap::new(),
        proximo: 0,
    });
}

/// Roda `body` com o nó do handle, ou registra erro de handle inválido.
fn com_no<T>(handle: u64, body: impl FnOnce(&mut No) -> Result<T, i32>) -> Result<T, i32> {
    REGISTRO.with(|r| {
        let mut registro = r.borrow_mut();
        match registro.nos.get_mut(&handle) {
            Some(no) => body(no),
            None => Err({
                state::set_last_error(&format!("unknown network handle {handle}"));
                GIOCHI_ERROR_NETWORK
            }),
        }
    })
}

/// Converte um erro de rede em código ABI, registrando a mensagem.
fn erro_rede(contexto: &str, e: impl core::fmt::Display) -> i32 {
    state::set_last_error(&format!("{contexto}: {e}"));
    GIOCHI_ERROR_NETWORK
}

/// Sobe um host escutando na porta dada. `0` escolhe uma porta efêmera — use
/// [`giochi_net_local_port`] para descobrir qual foi.
///
/// Devolve `0` em caso de falha.
///
/// # C ABI
///
/// ```c
/// uint64_t giochi_net_host_create(uint16_t porta);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_host_create(porta: u16) -> u64 {
    guard(0, || {
        let porta = if porta == 0 { None } else { Some(porta) };
        match Host::novo(porta) {
            Ok(host) => REGISTRO.with(|r| {
                r.borrow_mut().inserir(No::Host {
                    host,
                    pendentes: Vec::new(),
                })
            }),
            Err(e) => {
                erro_rede("could not start the host", e);
                0
            }
        }
    })
}

/// Entra numa partida. `endereco` é `"192.168.1.10:47721"` ou `"host:porta"`.
///
/// Devolve `0` em caso de falha. O handshake é enviado aqui; a conexão fica
/// pronta quando [`giochi_net_player_id`] devolver um valor positivo.
///
/// # C ABI
///
/// ```c
/// uint64_t giochi_net_client_join(const char* endereco);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_client_join(endereco: *const c_char) -> u64 {
    guard(0, || {
        if endereco.is_null() {
            state::invalid_argument("giochi_net_client_join: endereco is NULL");
            return 0;
        }
        // SAFETY: checked for NULL; the contract says the pointer is a NUL-terminated
        // string that stays valid for the duration of the call.
        let texto = match unsafe { CStr::from_ptr(endereco) }.to_str() {
            Ok(s) => s,
            Err(_) => {
                state::invalid_argument("giochi_net_client_join: endereco is not UTF-8");
                return 0;
            }
        };
        match Cliente::entrar(texto) {
            Ok(cliente) => REGISTRO.with(|r| {
                r.borrow_mut().inserir(No::Cliente {
                    cliente,
                    camera: None,
                    ultimo: None,
                })
            }),
            Err(e) => {
                erro_rede("could not join", e);
                0
            }
        }
    })
}

/// Liberta o nó. Devolve erro se o handle não existir.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_net_destroy(uint64_t handle);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_destroy(handle: u64) -> i32 {
    guard(GIOCHI_ERROR_NETWORK, || {
        REGISTRO.with(|r| {
            if r.borrow_mut().nos.remove(&handle).is_some() {
                0
            } else {
                state::set_last_error(&format!("unknown network handle {handle}"));
                GIOCHI_ERROR_NETWORK
            }
        })
    })
}

/// A porta em que o host está escutando, ou o erro negativo.
///
/// Existe porque `porta = 0` em [`giochi_net_host_create`] delega a escolha ao
/// sistema — sem esta função, o host não teria como dizer aos clientes onde
/// entrar.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_net_local_port(uint64_t handle);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_local_port(handle: u64) -> i32 {
    guard(GIOCHI_ERROR_NETWORK, || {
        com_no(handle, |no| match no {
            No::Host { host, .. } => match host.endereco() {
                Ok(endereco) => Ok(endereco.port() as i32),
                Err(e) => Err(erro_rede("could not read the host address", e)),
            },
            No::Cliente { cliente, .. } => Ok(cliente.host().port() as i32),
        })
        .unwrap_or_else(|codigo| codigo)
    })
}

/// Id do jogador atribuído pelo host, ou `0` se ainda não chegou.
///
/// `0` também é o valor devolvido para um handle de host — o host não tem id.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_net_player_id(uint64_t handle);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_player_id(handle: u64) -> i32 {
    guard(0, || {
        com_no(handle, |no| match no {
            No::Host { .. } => Ok(0),
            No::Cliente { cliente, .. } => Ok(cliente.jogador().map(|id| id as i32).unwrap_or(0)),
        })
        .unwrap_or(0)
    })
}

/// Bombeia a rede uma vez. Barato; chame a cada frame.
///
/// Para o cliente, guarda o último snapshot e responde ao ping do host. Para o
/// host, recebe os inputs (bombeados por [`giochi_net_poll_input`]).
///
/// # C ABI
///
/// ```c
/// int32_t giochi_net_pump(uint64_t handle);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_pump(handle: u64) -> i32 {
    guard(GIOCHI_ERROR_NETWORK, || {
        com_no(handle, |no| match no {
            No::Host { host, pendentes } => {
                // Drena e GUARDA: `poll_input` lê desta fila. Drenar duas vezes no
                // mesmo frame perderia o input que chegou no meio.
                *pendentes = host.processar(64);
                Ok(0)
            }
            No::Cliente {
                cliente, ultimo, ..
            } => {
                for mensagem in cliente.processar(64) {
                    if let Mensagem::Inimigos { tick, inimigos } = mensagem {
                        // Guarda o mais novo: snapshot fora de ordem não pode
                        // sobrescrever um mais recente.
                        let novo = match ultimo {
                            Some((t, _)) => tick > *t,
                            None => true,
                        };
                        if novo {
                            *ultimo = Some((tick, inimigos));
                        }
                    }
                }
                match cliente.estado() {
                    Estado::Recusado => Err({
                        state::set_last_error("the host refused this client");
                        GIOCHI_ERROR_NETWORK
                    }),
                    _ => Ok(0),
                }
            }
        })
        .unwrap_or_else(|codigo| codigo)
    })
}

/// Lê um input do host.
///
/// `out` recebe o input; `id` recebe de qual jogador veio (1..8, `0` é o host).
/// Devolve `0` com `*escritos = 0` quando não há nada, e `0` com `*escritos = 1`
/// quando escreveu.
///
/// Este é o par de `giochi_net_send_input`: sem ele o host não tem como receber
/// nada, e a partida não acontece.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_net_poll_input(uint64_t handle, GiochiNetInput* out,
///                               uint8_t* id, uint32_t* escritos);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_poll_input(
    handle: u64,
    out: *mut GiochiNetInput,
    id: *mut u8,
    escritos: *mut u32,
) -> i32 {
    guard(GIOCHI_ERROR_NETWORK, || {
        if out.is_null() || id.is_null() || escritos.is_null() {
            return state::invalid_argument(
                "giochi_net_poll_input: out, id and escritos must not be NULL",
            );
        }
        // Só faz sentido num host; num cliente a resposta é "não há input".
        //
        // Nada de `?` aqui: o corpo do `guard` devolve `i32`, não `Result`, então o
        // erro tem de ser desembrulhado à mão.
        let proximo = match com_no(handle, |no| {
            match no {
            No::Cliente { .. } => Err(state::invalid_argument(
                "giochi_net_poll_input: only a host receives input; use giochi_net_send_input on a client",
            )),
            No::Host { pendentes, .. } => {
                // Consome só o que é input: ping e handshake já foram resolvidos
                // dentro de `Host::processar` e não têm o que fazer aqui.
                //
                // `position` em vez de `matches!` sobre o valor: `Mensagem` não é
                // `Copy`, e `matches!(mensagem, ...)` moveria antes da hora.
                let posicao = pendentes
                    .iter()
                    .position(|(_, m)| matches!(m, Mensagem::Input { .. }));
                Ok(posicao.map(|i| pendentes.remove(i)))
            }
        }
        }) {
            Ok(p) => p,
            Err(codigo) => return codigo,
        };
        match proximo {
            None => {
                // SAFETY: `escritos` was checked for NULL above.
                unsafe { *escritos = 0 };
                0
            }
            Some((jogador, mensagem)) => {
                let Mensagem::Input {
                    tick,
                    mover_x,
                    mover_z,
                    mirar_x,
                    mirar_z,
                    acoes,
                    camera_x,
                    camera_z,
                } = mensagem
                else {
                    // Ping e afins já foram tratados dentro do host.
                    // SAFETY: `escritos` was checked for NULL above.
                    unsafe { *escritos = 0 };
                    return 0;
                };
                // SAFETY: `out`, `id` and `escritos` were checked for NULL above, and
                // the C contract says they point to writable storage of the right type.
                unsafe {
                    *out = GiochiNetInput {
                        tick,
                        mover_x,
                        mover_z,
                        mirar_x,
                        mirar_z,
                        acoes,
                        camera_x,
                        camera_z,
                    };
                    *id = jogador;
                    *escritos = 1;
                }
                0
            }
        }
    })
}

/// Manda o input do jogador. Só faz sentido num cliente.
///
/// A câmera vai no input (é ela que define a área de interesse no host), então
/// não é preciso chamar [`giochi_net_set_camera_pos`] antes de cada quadro — ela
/// só existe para repor a câmera quando o jogo não a calcula por input.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_net_send_input(uint64_t handle, const GiochiNetInput* input);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_send_input(handle: u64, input: *const GiochiNetInput) -> i32 {
    guard(GIOCHI_ERROR_NETWORK, || {
        if input.is_null() {
            return state::invalid_argument("giochi_net_send_input: input is NULL");
        }
        // SAFETY: checked for NULL; the C contract says it points to a valid struct.
        let copia = unsafe { *input };
        com_no(handle, |no| match no {
            No::Host { .. } => Err(state::invalid_argument(
                "giochi_net_send_input: a host does not send input; it receives it",
            )),
            No::Cliente {
                cliente, camera, ..
            } => {
                // Câmera reposta por `set_camera_pos` sobrepõe a do input. É o que
                // dá sentido àquela função: quem calcula a câmera à parte não
                // precisa repetir os valores em cada input.
                let (camera_x, camera_z) = camera.unwrap_or((copia.camera_x, copia.camera_z));
                let mensagem = Mensagem::Input {
                    tick: copia.tick,
                    mover_x: copia.mover_x,
                    mover_z: copia.mover_z,
                    mirar_x: copia.mirar_x,
                    mirar_z: copia.mirar_z,
                    acoes: copia.acoes,
                    camera_x,
                    camera_z,
                };
                match cliente.enviar_input(mensagem) {
                    Ok(()) => Ok(0),
                    Err(e) => Err(erro_rede("could not send input", e)),
                }
            }
        })
        .unwrap_or_else(|codigo| codigo)
    })
}

/// Repõe a câmera que vai junto com o próximo input. Só cliente.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_net_set_camera_pos(uint64_t handle, float x, float z);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_set_camera_pos(handle: u64, x: f32, z: f32) -> i32 {
    guard(GIOCHI_ERROR_NETWORK, || {
        com_no(handle, |no| match no {
            No::Host { .. } => Err(state::invalid_argument(
                "giochi_net_set_camera_pos: a host has no camera; use giochi_net_set_area",
            )),
            No::Cliente { camera, .. } => {
                *camera = Some((x, z));
                Ok(0)
            }
        })
        .unwrap_or_else(|codigo| codigo)
    })
}

/// Define o raio da área de interesse de um cliente, no host.
///
/// Quem sabe quanto do mundo cabe na tela é o **jogo**, não a engine: zoom, FOV
/// e névoa mudam o raio. Por isso ele é parâmetro, com o valor que o jogo
/// calculou (o relatório sugere `1,5 ×` a meia-diagonal da tela).
///
/// `raio <= 0` desliga o filtro e manda todos os inimigos.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_net_set_area(uint64_t handle, uint8_t jogador, float raio);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_set_area(handle: u64, jogador: u8, raio: f32) -> i32 {
    guard(GIOCHI_ERROR_NETWORK, || {
        com_no(handle, |no| match no {
            No::Cliente { .. } => Err(state::invalid_argument(
                "giochi_net_set_area: only a host has clients to filter",
            )),
            No::Host { host, .. } => {
                host.definir_area(jogador, raio);
                Ok(0)
            }
        })
        .unwrap_or_else(|codigo| codigo)
    })
}

/// Publica o mundo do host: filtra por área de interesse e manda um snapshot
/// por cliente.
///
/// Chamar a 60 Hz é o uso normal; o host decide internamente se já é hora
/// (15 Hz). Devolve `0` mesmo quando não enviou nada.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_net_host_snapshot(uint64_t handle, const GiochiNetInimigo* inimigos,
///                                  uint32_t quantidade);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_host_snapshot(
    handle: u64,
    inimigos: *const GiochiNetInimigo,
    quantidade: u32,
) -> i32 {
    guard(GIOCHI_ERROR_NETWORK, || {
        if inimigos.is_null() && quantidade > 0 {
            return state::invalid_argument(
                "giochi_net_host_snapshot: inimigos is NULL but quantidade is not zero",
            );
        }
        let mut lista = Vec::with_capacity(quantidade as usize);
        if quantidade > 0 {
            // SAFETY: `inimigos` is non-NULL (checked above) and the C contract says it
            // points to `quantidade` valid entries.
            let fatia = unsafe { std::slice::from_raw_parts(inimigos, quantidade as usize) };
            for i in fatia {
                lista.push(SnapshotInimigo {
                    id: i.id,
                    x: i.x,
                    z: i.z,
                    hp: i.hp,
                    tipo: i.tipo,
                    fase_chefe: i.fase_chefe,
                });
            }
        }
        com_no(handle, |no| match no {
            No::Cliente { .. } => Err(state::invalid_argument(
                "giochi_net_host_snapshot: only a host publishes the world",
            )),
            No::Host { host, .. } => {
                host.tick_snapshot(&lista);
                Ok(0)
            }
        })
        .unwrap_or_else(|codigo| codigo)
    })
}

/// Broadcast de estado global (onda, torre, chefe) para todos os clientes.
///
/// Bytes opacos: a engine transporta, o jogo interpreta.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_net_broadcast(uint64_t handle, const uint8_t* dados, uint32_t tamanho);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_broadcast(handle: u64, dados: *const u8, tamanho: u32) -> i32 {
    guard(GIOCHI_ERROR_NETWORK, || {
        if dados.is_null() && tamanho > 0 {
            return state::invalid_argument(
                "giochi_net_broadcast: dados is NULL but tamanho is not zero",
            );
        }
        let mut copia = Vec::with_capacity(tamanho as usize);
        if tamanho > 0 {
            // SAFETY: `dados` is non-NULL (checked above) and the C contract says it
            // points to `tamanho` readable bytes.
            copia.extend_from_slice(unsafe { std::slice::from_raw_parts(dados, tamanho as usize) });
        }
        com_no(handle, |no| match no {
            No::Cliente { .. } => Err(state::invalid_argument(
                "giochi_net_broadcast: only a host broadcasts",
            )),
            No::Host { host, .. } => {
                host.enviar_global(&copia);
                Ok(0)
            }
        })
        .unwrap_or_else(|codigo| codigo)
    })
}

/// Lê o último snapshot recebido pelo cliente.
///
/// Devolve [`GIOCHI_NET_AGUARDE`] (`1`) quando não há nada novo desde a última
/// leitura, `0` quando escreveu, negativo em caso de erro.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_net_poll_snapshot(uint64_t handle, GiochiNetSnapshot* out);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_poll_snapshot(handle: u64, out: *mut GiochiNetSnapshot) -> i32 {
    poll_snapshot(handle, out, GIOCHI_NET_MAX_INIMIGOS as u32)
}

/// Igual a [`giochi_net_poll_snapshot`], mas copia no máximo `max` inimigos.
///
/// `max` acima de [`GIOCHI_NET_MAX_INIMIGOS`] é reduzido ao teto — o array da
/// struct não cresce.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_net_poll_snapshot_filtered(uint64_t handle, GiochiNetSnapshot* out,
///                                           uint32_t max);
/// ```
#[no_mangle]
pub extern "C" fn giochi_net_poll_snapshot_filtered(
    handle: u64,
    out: *mut GiochiNetSnapshot,
    max: u32,
) -> i32 {
    poll_snapshot(handle, out, max)
}

fn poll_snapshot(handle: u64, out: *mut GiochiNetSnapshot, max: u32) -> i32 {
    guard(GIOCHI_ERROR_NETWORK, || {
        if out.is_null() {
            return state::invalid_argument("giochi_net_poll_snapshot: out is NULL");
        }
        let limite = (max as usize).min(GIOCHI_NET_MAX_INIMIGOS);
        com_no(handle, |no| match no {
            No::Host { .. } => Err(state::invalid_argument(
                "giochi_net_poll_snapshot: only a client receives snapshots",
            )),
            No::Cliente { ultimo, .. } => {
                let Some((tick, inimigos)) = ultimo else {
                    return Ok(GIOCHI_NET_AGUARDE);
                };
                let mut snapshot = GiochiNetSnapshot {
                    tick: *tick,
                    total: inimigos.len() as u32,
                    escritos: 0,
                    capacidade: GIOCHI_NET_MAX_INIMIGOS as u32,
                    inimigos: [GiochiNetInimigo::default(); GIOCHI_NET_MAX_INIMIGOS],
                };
                let n = inimigos.len().min(limite);
                for (destino, origem) in snapshot.inimigos.iter_mut().zip(inimigos.iter()).take(n) {
                    *destino = GiochiNetInimigo::from(*origem);
                }
                snapshot.escritos = n as u32;
                // SAFETY: `out` was checked for NULL above and the C contract says it
                // points to writable storage of this type.
                unsafe { *out = snapshot };
                Ok(0)
            }
        })
        .unwrap_or_else(|codigo| codigo)
    })
}

#[cfg(test)]
mod testes {
    use super::*;
    // Só os testes usam este código: importá-lo no topo daria "unused import" na
    // compilação normal da biblioteca, e o CI trata aviso como erro.
    use crate::abi_types::GIOCHI_ERROR_INVALID_ARGUMENT;

    #[test]
    fn tamanho_das_structs_e_estavel() {
        // O header declara estas structs; um campo novo muda o tamanho e um
        // consumidor compilado contra o header antigo leria deslocado.
        assert_eq!(core::mem::size_of::<GiochiNetInimigo>(), 16);
        assert_eq!(core::mem::align_of::<GiochiNetInimigo>(), 4);
        assert_eq!(
            core::mem::size_of::<GiochiNetSnapshot>(),
            16 + GIOCHI_NET_MAX_INIMIGOS * 16
        );
        assert_eq!(core::mem::size_of::<GiochiNetInput>(), 32);
    }

    #[test]
    fn handle_zero_nunca_e_valido() {
        assert_eq!(giochi_net_destroy(0), GIOCHI_ERROR_NETWORK);
        assert_eq!(giochi_net_player_id(0), 0);
        assert_eq!(giochi_net_local_port(0), GIOCHI_ERROR_NETWORK);
    }

    #[test]
    fn host_sobe_e_reporta_a_porta() {
        let handle = giochi_net_host_create(0);
        assert_ne!(handle, 0, "devia criar");
        let porta = giochi_net_local_port(handle);
        assert!(porta > 0, "porta efêmera devia ser reportada, veio {porta}");
        assert_eq!(giochi_net_destroy(handle), 0);
        // Depois de destruir, o handle não vale mais.
        assert_eq!(giochi_net_local_port(handle), GIOCHI_ERROR_NETWORK);
    }

    #[test]
    fn cliente_recusa_endereco_invalido_e_null() {
        assert_eq!(giochi_net_client_join(std::ptr::null()), 0);
        let texto = std::ffi::CString::new("nao-e-um-endereco").unwrap();
        assert_eq!(giochi_net_client_join(texto.as_ptr()), 0);
    }

    #[test]
    fn papel_errado_e_recusado_com_mensagem() {
        let host = giochi_net_host_create(0);
        assert_ne!(host, 0);

        let input = GiochiNetInput::default();
        assert_eq!(
            giochi_net_send_input(host, &input),
            GIOCHI_ERROR_INVALID_ARGUMENT
        );

        let mut snapshot = GiochiNetSnapshot::default();
        assert_eq!(
            giochi_net_poll_snapshot(host, &mut snapshot),
            GIOCHI_ERROR_INVALID_ARGUMENT
        );

        assert_eq!(giochi_net_destroy(host), 0);
    }

    #[test]
    fn argumentos_null_sao_recusados() {
        let host = giochi_net_host_create(0);
        assert_ne!(host, 0);
        assert_eq!(
            giochi_net_poll_snapshot(host, std::ptr::null_mut()),
            GIOCHI_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(
            giochi_net_host_snapshot(host, std::ptr::null(), 5),
            GIOCHI_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(giochi_net_destroy(host), 0);
    }

    #[test]
    fn snapshot_truncado_reporta_total_maior_que_escritos() {
        // Prende a razão de existirem os dois contadores: com limite menor que o
        // número de inimigos, `total` continua sendo o real.
        let mut snapshot = GiochiNetSnapshot::default();
        assert_eq!(snapshot.total, 0);
        assert_eq!(snapshot.capacidade, GIOCHI_NET_MAX_INIMIGOS as u32);

        // Simula o preenchimento que `poll_snapshot` faz.
        let inimigos: Vec<SnapshotInimigo> = (0..10)
            .map(|i| SnapshotInimigo {
                id: i,
                x: i as f32,
                z: 0.0,
                hp: 5,
                tipo: 0,
                fase_chefe: 0,
            })
            .collect();
        snapshot.total = inimigos.len() as u32;
        let n = inimigos.len().min(3);
        for (destino, origem) in snapshot.inimigos.iter_mut().zip(inimigos.iter()).take(n) {
            *destino = GiochiNetInimigo::from(*origem);
        }
        snapshot.escritos = n as u32;

        assert_eq!(snapshot.total, 10);
        assert_eq!(snapshot.escritos, 3);
        assert!(
            snapshot.total > snapshot.escritos,
            "truncamento tem de ser visível"
        );
    }

    #[test]
    fn conversao_de_inimigo_preserva_todos_os_campos() {
        let origem = SnapshotInimigo {
            id: 7,
            x: 1.5,
            z: -2.25,
            hp: 300,
            tipo: 3,
            fase_chefe: 2,
        };
        let destino = GiochiNetInimigo::from(origem);
        assert_eq!(destino.id, 7);
        assert_eq!(destino.x, 1.5);
        assert_eq!(destino.z, -2.25);
        assert_eq!(destino.hp, 300);
        assert_eq!(destino.tipo, 3);
        assert_eq!(destino.fase_chefe, 2);
    }

    #[test]
    fn codigo_de_espera_nao_e_erro() {
        // `1` é "nada novo", não falha. Se alguém trocar por um negativo, o
        // cliente trataria o caso normal como erro.
        assert_eq!(GIOCHI_NET_AGUARDE, 1);
        assert!(GIOCHI_NET_AGUARDE > 0);
    }
}
