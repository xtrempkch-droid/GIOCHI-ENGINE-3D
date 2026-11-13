# Rede P2P — `giochi-net`

Implementação do **Relatório 2** do
[Relatório consultor externo](../Relatório%20consultor%20externo.txt): camada de
rede P2P host-autoritativa com *Interest Management*, para o GIOCHI-OS em Linux
ARM64.

> **Estado: compilado e testado.** `cargo test -p giochi-net` dá **34/34**, e
> **38/38** com `--features laminar`; `clippy -D warnings` e `fmt --check`
> passam. O que **não** foi feito está na §3: a ABI C, o teste com dois
> processos e a validação de cross-compile no CI.

---

## 1. O que a engine ganhou

| Item | Arquivo | Estado |
| --- | --- | --- |
| protocolo binário, 10 variantes | `src/wire.rs` | escrito |
| filtro de área de interesse | `src/aoi.rs` | escrito |
| socket não-bloqueante + dreno com orçamento | `src/peer.rs` | escrito |
| host autoritativo, snapshot por cliente | `src/host.rs` | escrito |
| cliente (handshake, input, RTT) | `src/client.rs` | escrito |

`giochi-net` entrou como membro do workspace, com **zero dependência** — nem
`serde`.

---

## 2. As quatro decisões que divergem do relatório

### 2.1 Serialização escrita à mão, não `postcard`/`bincode`

O relatório deixa em aberto entre `postcard` e `bincode`. Nenhum dos dois é usado
aqui, e o motivo é o mesmo requisito que o relatório impõe: compilar para
`aarch64-unknown-linux-gnu` sem dependência de sistema.

Ambos exigem `serde` + derive. O protocolo tem 10 variantes de tipo fixo — a
tabela escrita à mão em `wire.rs` é menor que o código que o derive geraria, e
dá duas coisas que o derive não dá:

- **offset exato** do erro (`Truncado { offset, faltam }`), em vez de "pacote inválido";
- **recusa de versão antes de ler o corpo**, então cliente antigo e host novo não
  trocam estado pela metade.

**O custo é real:** o layout é manual, então mexer num campo exige mexer em quatro
lugares (struct, `codificar`, `decodificar`, teste). É o preço de não ter
dependência.

### 2.2 O raio da AOI é do jogo, não da engine

O relatório diz "AOI = raio de ~1,5 × meia-diagonal da tela do cliente". Essa
fórmula depende de quanto do mundo o jogador vê, que é decisão de câmera e de
design — a engine não pode saber.

`AreaDeInteresse` recebe o raio pronto, e `Host::definir_area(id, raio)` deixa o
jogo informar. `AreaDeInteresse::da_tela(...)` existe como atalho para quem quiser
a fórmula do relatório, mas é opção, não padrão.

### 2.3 Dois transportes: o próprio e o `laminar`

O transporte próprio (`src/peer.rs`) é um `UdpSocket` **sem garantia nenhuma**.
Datagrama perdido é perdido. Isso serve para snapshot de posição — o próximo
tick corrige — e **não** serve para handshake, compra na loja ou estado global:
perder um `ShopBuy` é uma compra que o jogador fez e não aconteceu.

Por isso entrou o **`laminar` 0.5** (a camada de rede do Amethyst, usada em
jogos), como **feature opcional** `laminar`:

| | transporte próprio | `laminar` |
| --- | --- | --- |
| dependências | **zero** | 12 crates |
| entrega garantida | não | sim (canais) |
| ordenação | não | sim, por canal |
| fragmentação | não (limite 1200 B) | sim |
| runtime assíncrono | não | não |
| aarch64 sem dependência | **sim** | não |

**O protocolo não muda.** Os mesmos bytes de `wire.rs` viajam por baixo; o que
muda é a garantia por variante, decidida em `laminar_backend::garantia_de`:

| Variante | Garantia | Por quê |
| --- | --- | --- |
| Handshake, HandshakeOk, Recusado | confiável | perder = não entrar na partida |
| ShopBuy | confiável | compra perdida com ouro já gasto |
| Global (onda, torre, chefe) | confiável | HUD mentiria sobre o HP da torre |
| Input | sequenciada | input atrasado não serve; ordem importa |
| Jogador | sequenciada | fora de ordem faz o jogador "voltar" |
| Inimigos | descartável | o próximo tick substitui |
| Ping, Pong | descartável | perder só atrasa a medição |

Confiar no `laminar` troca "escrevi 200 linhas de protocolo" por "dependo de um
crate de 2021". É uma troca defensável — o que ele faz (reliability, ordering,
fragmentação) é notoriamente difícil de acertar à mão —, mas é uma troca, e o
padrão continua sendo o transporte próprio para o alvo aarch64 poder ficar sem
dependência alguma.

### Um detalhe do `laminar` que custa uma sessão de depuração

`Socket::send` **só enfileira**. Quem transmite é o `manual_poll`, que roda
dentro do nosso `drenar`. Consequência: **os dois lados precisam chamar
`drenar` a cada frame**, inclusive quem só envia. Um host que só recebe e um
cliente que só envia — arranjo natural — faz o cliente nunca transmitir nada, e
o sintoma é "a rede não funciona" sem erro nenhum.

### 2.4 mDNS: entrou, com uma armadilha medida
O relatório pede descoberta automática em LAN. **Entrou**, atrás da feature
`mdns`, com `mdns-sd` 0.13 (ver `src/descobrir.rs`).

Custa a única dependência com peso do crate: `mdns-sd` puxa `if-addrs`,
`socket2`, `flume` e `mio`. Por isso fica **desligada por padrão** — o alvo
aarch64 continua podendo ser zero-dependência. O CI cross-verifica a feature
`mdns` a parte, em `scripts/check-aarch64.sh`.

**Por que não foi escrito à mão:** mDNS parece simples e não é. Sondagem para
evitar colisão de nome, *goodbye* com TTL 0, resposta a PTR/SRV/TXT/A, e — a que
mais dói — **enviar para o grupo multicast na interface certa**: um `send_to` em
`224.0.0.251` num socket sem `IP_MULTICAST_IF` sai pela rota padrão e ninguém na
LAN vê. Seriam sessenta linhas de socket option e um comportamento que só falha
no aparelho de outra pessoa.

**A armadilha medida, e que custa uma sessão:** anunciar com `127.0.0.1` não
funciona. O registro existe, o *browse* encontra, e **nenhum outro aparelho
consegue conectar**. Medido nesta máquina: com `127.0.0.1` a descoberta deu
"não achou" em 15 s; com o IP da interface (`192.168.1.107`), resolveu em menos
de 2 s. `ip_anunciavel()` descobre o IP certo com o truque do `connect` UDP e
**recusa loopback** — a recusa é a correção.

Se o cross-compile de `mdns` falhar em aarch64, o substituto é **broadcast UDP**
num endereço fixo: 30 linhas, sem dependência, funciona em LAN plana.

---

## 3. O que o relatório pede e o estado de cada item

| # | Pedido | Estado |
| --- | --- | --- |
| 1 | crate `giochi-net` | **feito** |
| 2 | protocolo + máquina de estados | **feito** |
| 3 | funções na ABI C (`giochi-engine-capi`) | **feito** — parte IV, ABI v4 (commit `45e2ffa`) |
| 4 | teste de integração com dois processos | **parcial** — host e cliente rodam em sockets reais dentro do mesmo processo de teste; dois processos não |
| 5 | cross-compile aarch64 no CI | **feito** — `scripts/check-aarch64.sh` cobre o workspace e as features `laminar` e `mdns` |
| — | AOI por cliente | **feito** |
| — | `net_set_camera_pos` / `net_poll_snapshot_filtered` | **feito** na ABI |
| — | descoberta em LAN (mDNS) | **feito** — feature `mdns`, §2.4 |

### O que sobrou da ABI C

Nada. As 14 funções estão em `giochi-engine-capi/src/net.rs`, o header foi
regerado com `cbindgen 0.27`, e o contrato ganhou §16–19. Verificado de C,
Python e Rust no CI (`c_smoke: OK (abi 4, ...)`).

**A descoberta mDNS não está na ABI**, de propósito: ela serve ao *launcher*
(Rust), que encontra o host e passa o endereço pronto ao app. Expor ao C
significaria mais três funções e uma ABI v5 sem necessidade demonstrada. Se um
app C precisar descobrir sozinho, é aí que se faz.

### Como a ABI C foi feita (e o que custou)

O `giochi-engine-capi` usa registry de handles opacos `u64` e `#[repr(C)]`, com
`cbindgen` gerando o header. A parte IV seguiu o mesmo padrão, e o obstáculo real
não foi nenhum dos três previstos — foi **C não ter `Vec`**:

`Mensagem::Inimigos` carrega uma lista, e uma struct `#[repr(C)]` precisa de
tamanho fixo. A saída foi array de 64 mais **dois contadores** (`total` e
`escritos`), e `total > escritos` passou a ser o sinal de truncamento que o
consumidor tem de olhar.

Uma exceção deliberada à regra "0 ou negativo": `giochi_net_poll_snapshot`
devolve `1` (`GIOCHI_NET_AGUARDE`) quando não há snapshot novo, porque o host
publica a 15 Hz e o jogo chama a 60 Hz — esse é o caso normal, não erro.

O teste de layout não foi tocado: ele prende as structs do 2D/3D, e a parte IV
tem asserts próprios em `net.rs::tamanho_das_structs_e_estavel`.

---

## 4. O que os testes cobrem

`cargo test -p giochi-net` — **34 testes**; **38** com `--features laminar`;
**37 + 1 ignorado** com `--features mdns`. Todos executados:

| Configuração | Testes | Observação |
| --- | --- | --- |
| padrão | 34 | zero dependência |
| `--features laminar` | 38 | +4 do transporte |
| `--features mdns` | 37 | +3 do mDNS e 1 `#[ignore]` |

O teste ignorado (`encontra_o_proprio_anuncio`) **é** executado, com
`-- --ignored`: depende de multicast numa interface real, então não pode rodar
no CI. Medido nesta máquina: encontra o próprio anúncio em **1,03 s**.

| Arquivo | Testes | O que fixa |
| --- | --- | --- |
| `wire.rs` | 8 | ida e volta das 10 variantes, versão errada recusada, truncado com offset, tag inválida, snapshot truncado sem corromper, quantização saturando, payload grande |
| `aoi.rs` | 8 | centro vê a região e não o mundo, fora não vê nada, borda inclusiva, raio 0 desliga (não esconde), raio negativo/NaN, fórmula da tela, buffer reaproveitado, a conta do relatório |
| `peer.rs` | 6 | abre em porta efêmera, **não bloqueia** (mede tempo), envia e recebe, lixo vira evento e não derruba o socket, orçamento limita |
| `host.rs` | 5 | sobe e escuta, handshake atribui id 1, versão errada não cria cliente, input sem handshake ignorado, snapshot respeita taxa e AOI |
| `client.rs` | 5 | entra e recebe id, input chega e snapshot volta filtrado, `enviar_input` recusa o que não é do cliente, pacote de outro endereço ignorado, RTT satura |
| `lib.rs` | 2 | versão/porta/taxa estáveis, datagrama cabe no MTU mínimo de IPv6 |
| `descobrir.rs` (feature `mdns`) | 3 + 1 ignorado | nome de instância utilizável, tipo de serviço com o sufixo obrigatório, `ip_anunciavel` recusa loopback; o ignorado anuncia e encontra o próprio anúncio de verdade |

O teste mais importante é o de `peer.rs`: ele **mede o tempo** de `drenar` sem
pacote na fila. Se o socket voltasse a ser bloqueante, ele falha por timeout — é o
único jeito de provar "não bloqueia" em vez de afirmar.

---

## 5. Dois defeitos que a revisão pegou antes de existir compilador

Registrados porque são a classe de erro que só aparece rodando:

**1. `HandshakeRecusado` estourava o buffer no decode.**
O tamanho do motivo vem do fio como `u8` (até 255) e era copiado para um array de
48 bytes — `copy_from_slice` com tamanho maior **entra em pânico**. Um datagrama
forjado derrubaria o host. Corrigido com `n.min(48)`.

**2. O RTT do ping era sempre ~0.**
A marca enviada no `Ping` vinha de `agora.elapsed()`, onde `agora` era capturado no
início da mesma função — ou seja, ~0 ms sempre. O RTT sairia zero: número
plausível e falso, que é pior que número ausente. Corrigido com um `Instant`
guardado na criação do cliente.

E um terceiro, de estrutura: o `Cliente` chamava `Mensagem::ShopBuy`, variante que
não existia no enum. Compilar teria falhado na primeira linha útil.

---

## 6. O que fazer na próxima sessão, em ordem

1. **Teste com dois processos** (§4.2 do `TRANSFERENCIA.md`) — é o único item do
   relatório ainda incompleto. Host e cliente já trocam pacotes em sockets reais,
   mas no mesmo processo.
2. **Rodar em hardware.** Nada disto rodou num aparelho. Os números de AOI em
   §2.3 são aritmética, não medição.
3. **Permissão de rede no manifesto** — cruza `Giochi app` e `Giochi os`;
   documentado em §7.
4. **Decidir se a descoberta vai para a ABI C.** Hoje é só Rust, e o launcher é
   Rust — mas se um app C precisar descobrir sozinho, são três funções e ABI v5.
5. **Reavaliar `mdns-sd` 0.21.** A 0.13 foi a que se verificou; a 0.21 existe e é
   mais nova. Trocar exige repetir esta bateria (<code>--features mdns</code> e o
   cross-check aarch64).

## 7. O que o GIOCHI-OS precisa, e ainda não tem

O relatório diz que o manifesto do app já declara permissão de rede. **Não
declara.** Verificado neste projeto:

- `Giochi os/scripts/assemble.mjs` lê `apps.json` + `metadata.json` e empacota —
  não há campo de permissão, nem porta UDP, nem conceito de manifesto de
  permissões;
- `Giochi app/schemas/metadata.schema.json` é `additionalProperties: false` e não
  tem `network`.

Então declarar rede exige mexer em **dois repositórios** (schema + validador no
`Giochi app`, e o empacotador no `Giochi os`) e depois no sistema, que é quem
poderia abrir porta no firewall. Registrado aqui para não virar surpresa: o app
pode funcionar em rede hoje porque o aparelho não tem firewall — o que não é uma
decisão, é ausência de controle.
