# Rede P2P — `giochi-net`

Implementação do **Relatório 2** do
[Relatório consultor externo](../Relatório%20consultor%20externo.md): camada de
rede P2P host-autoritativa com *Interest Management*, para o GIOCHI-OS em Linux
ARM64.

> **Estado: escrito, NÃO compilado.** Não há `rustc` nem `cargo` na máquina onde
> este crate foi escrito. Nada abaixo foi executado — nem `cargo build`, nem
> `cargo test`. Trate como código a compilar, não como código a funcionar. O que
> o repositório tem de verdade são os testes escritos, que só valem depois de
> rodarem.

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

## 2. As três decisões que divergem do relatório

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

### 2.3 `mdns-sd` não entrou — e é decisão em aberto, não esquecimento

O relatório pede descoberta automática por mDNS. O `mdns-sd` é puro Rust, mas
puxa `if-addrs` e `socket2`, que enumeram interfaces por syscall. Pode passar em
`aarch64-unknown-linux-gnu` sem problema — **não há como saber sem compilar**, e
aqui não há compilador.

Adiar uma dependência por falta de ferramenta de verificação é decisão, e ela
custa: **sem mDNS, o cliente precisa do IP do host na mão.** No GIOCHI-OS isso é
aceitável no curto prazo (a rede é LAN, e o `giochi-selftest` já mostra o IP no
log), mas não é o que o relatório pediu.

Quando houver toolchain: ligar `mdns-sd`, compilar para aarch64, e se passar,
manter. Se falhar, o substituto é descoberta por **broadcast UDP** num endereço
fixo — 30 linhas, sem dependência, funciona em LAN plana.

---

## 3. O que o relatório pede e NÃO foi feito

| # | Pedido | Estado |
| --- | --- | --- |
| 1 | crate `giochi-net` | **feito** |
| 2 | protocolo + máquina de estados | **feito** |
| 3 | funções na ABI C (`giochi-engine-capi`) | **não feito** |
| 4 | teste de integração com dois processos | **parcial** — host e cliente rodam em sockets reais dentro do mesmo processo de teste; dois processos não |
| 5 | cross-compile aarch64 no CI | **não feito** |
| — | AOI por cliente | **feito** |
| — | `net_set_camera_pos` / `net_poll_snapshot_filtered` | **não feito** (a AOI existe na engine; a ABI não expõe) |

### Por que a ABI C não foi feita

O `giochi-engine-capi` usa registry de handles opacos `u64` e `#[repr(C)]`, com
`cbindgen` gerando o header e um teste de layout de ABI (`tests/abi_layout.rs`).
Acrescentar seis funções lá exige:

1. decidir a struct `#[repr(C)]` do snapshot (e ela precisa de **tamanho fixo**,
   porque C não tem `Vec`);
2. regenerar o header e o `C_ABI_CONTRACT.md`;
3. atualizar o teste de layout, que falha de propósito quando a ABI muda.

É trabalho de uma sessão própria, e fazer às cegas — sem compilar — produziria
header e documentação que não correspondem ao código. **A camada Rust está pronta;
a ponte C é o próximo passo**, e é o que destrava o jogo de verdade.

---

## 4. O que os testes cobrem (quando rodarem)

`cargo test -p giochi-net` — **34 testes escritos**:

| Arquivo | Testes | O que fixa |
| --- | --- | --- |
| `wire.rs` | 8 | ida e volta das 10 variantes, versão errada recusada, truncado com offset, tag inválida, snapshot truncado sem corromper, quantização saturando, payload grande |
| `aoi.rs` | 8 | centro vê a região e não o mundo, fora não vê nada, borda inclusiva, raio 0 desliga (não esconde), raio negativo/NaN, fórmula da tela, buffer reaproveitado, a conta do relatório |
| `peer.rs` | 6 | abre em porta efêmera, **não bloqueia** (mede tempo), envia e recebe, lixo vira evento e não derruba o socket, orçamento limita |
| `host.rs` | 5 | sobe e escuta, handshake atribui id 1, versão errada não cria cliente, input sem handshake ignorado, snapshot respeita taxa e AOI |
| `client.rs` | 5 | entra e recebe id, input chega e snapshot volta filtrado, `enviar_input` recusa o que não é do cliente, pacote de outro endereço ignorado, RTT satura |
| `lib.rs` | 2 | versão/porta/taxa estáveis, datagrama cabe no MTU mínimo de IPv6 |

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

1. **`cargo test -p giochi-net`** — é o primeiro comando. Os 34 testes nunca
   rodaram, e o crate nunca compilou.
2. **`cargo build --workspace`** — garantir que entrar no workspace não quebrou
   os outros cinco crates.
3. **`scripts/check-aarch64.sh`** — o repo já tem o script de cross-compile; ele
   precisa incluir `giochi-net`.
4. **ABI C** — `net_host_create`, `net_client_join`, `net_send_input`,
   `net_set_camera_pos`, `net_poll_snapshot`, `net_poll_snapshot_filtered`.
   Definir a struct de snapshot com tamanho fixo e regenerar header + contrato.
5. **Decidir mDNS** com o compilador na mão (§2.3).

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
