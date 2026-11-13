# Transferência — o que falta na rede do GIOCHI-ENGINE-3D

> Documento escrito para **outra IA ou pessoa que vá continuar este trabalho**.
> Ele assume zero contexto sobre a sessão que o produziu.
>
> Versão: 2026-11-11, sobre `893d154`.

---

## 0. Resumo em cinco linhas

1. A engine não tinha rede. Agora tem o crate `giochi-net`: protocolo binário,
   AOI, host/cliente, e um transporte alternativo com garantias de entrega.
2. **38 testes passam**, `cargo clippy -D warnings` e `cargo fmt --check` também.
   O CI do repositório está **verde** nos quatro jobs.
3. O CI estava vermelho antes disto, por causa de dois bumps do dependabot
   (`glam` e `glow`) — isso foi corrigido junto.
4. **A ABI C não foi feita.** É o próximo passo e o que destrava o jogo de
   verdade. Sem ela, nada disto é alcançável de C ou do GIOCHI-OS.
5. Este documento lista as armadilhas que custaram tempo. **Leia a §2 antes de
   rodar qualquer comando** — metade delas é de ambiente, não de código.

---

## 1. Estado do repositório

### Commits desta série (de `6da968a` até `893d154`)

| Commit | O que fez | Verificado |
| --- | --- | --- |
| `49c5d63` | cria o crate `giochi-net`: `wire`, `aoi`, `peer`, `host`, `client` | não (sem compilador na hora) |
| `872cb16` | corrige os 13 pontos de `rustfmt` + 3 defeitos que o compilador achou | fmt/clippy/check |
| `3d44be1` | 34/34 testes: corrige 2 bugs reais e 1 teste errado | `cargo test` |
| `b5a8e16` | adapta `giochi-engine-3d` a glam 0.34 e glow 0.18 | probe + CI |
| `8cfb815` | transporte `laminar` opcional | 38 testes + clippy |
| `36546a0` | cross-compila a feature `laminar` para aarch64 no CI | CI |
| `893d154` | `check-aarch64.sh` de volta a LF | CI |

### Onde está cada coisa

```
giochi-net/
  Cargo.toml              zero dependência; `laminar` é opcional
  src/lib.rs              constantes (VERSAO_PROTOCOLO, DATAGRAMA_MAX, ...)
  src/wire.rs             protocolo: 10 variantes, serialização à mão
  src/aoi.rs              Interest Management (filtro por raio)
  src/peer.rs             UdpSocket não-bloqueante + dreno com orçamento
  src/host.rs             autoridade da partida, snapshot unicast por AOI
  src/client.rs           handshake, input com câmera, RTT
  src/laminar_backend.rs  transporte alternativo (feature `laminar`)
docs/REDE.md              decisões, comparações e o que não foi feito
```

### Testes (38 no total)

| Arquivo | Testes |
| --- | --- |
| `wire.rs` | 8 |
| `aoi.rs` | 8 |
| `peer.rs` | 6 |
| `host.rs` | 5 |
| `client.rs` | 5 |
| `lib.rs` | 2 |
| `laminar_backend.rs` | 4 (só com `--features laminar`) |

**Nenhum teste foi escrito depois do código para "passar"**: dois deles acharam
bugs reais (§3.1 e §3.2).

---

## 2. AMBIENTE — leia antes de tudo

Esta máquina é Windows, e a maior parte do tempo perdido nesta sessão foi aqui,
não no problema. Os itens abaixo são ordem de execução, não sugestões.

### 2.1 Não há toolchain por padrão; instale assim

```powershell
winget install --id Rustlang.Rustup -e --source winget --accept-package-agreements --accept-source-agreements --silent
# depois que o rustup-init terminar:
$env:USERPROFILE\.cargo\bin\rustup.exe default stable
```

### 2.2 O alvo MSVC NÃO CONSEGUE LINKAR aqui

`cargo test` falha com `linker link.exe not found`: o alvo MSVC exige o Visual
Studio Build Tools, que **não estão instalados** e são pesados.

**Não instale o Visual Studio.** Use o toolchain GNU, que linka com o
`rust-lld` embutido:

```powershell
$env:USERPROFILE\.cargo\bin\rustup.exe toolchain install stable-x86_64-pc-windows-gnu --profile minimal
$env:USERPROFILE\.cargo\bin\rustup.exe component add --toolchain stable-x86_64-pc-windows-gnu clippy rustfmt
```

Todos os comandos passam a usar `+stable-x86_64-pc-windows-gnu`.

### 2.3 NUNCA ponha o MinGW no PATH

Existe um MinGW instalado (`BrechtSanders.WinLibs.POSIX.UCRT`) em
`C:\Users\Jussara dias\AppData\Local\Microsoft\WinGet\Packages\...`.

**Ele é inútil e é uma armadilha por dois motivos:**

1. o caminho dele tem **espaço**, e o `ld.exe` dele não resolve os próprios
   arquivos: `ld.exe: cannot find C:/Users/Jussara: No such file or directory`;
2. com `gcc` no PATH, o Rust **troca** o `rust-lld` (que funciona) pelo
   `x86_64-w64-mingw32-gcc` (que falha).

Resultado: o mesmo `cargo test` que passava começa a falhar. Rode com PATH
limpo:

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;C:\Windows\system32;C:\Windows"
```

Isso basta para `giochi-net`, que não tem dependência C. Se um dia precisar
compilar C, instale o MinGW em `C:\mingw64` (sem espaço) — **nunca** em
`C:\Users\...`.

### 2.4 Fins de linha: `core.autocrlf` já está `false`, não mexa

O repositório guarda **LF**. Esta máquina veio com `core.autocrlf=true`, o que
fez o `rustfmt` reprovar 127 arquivos com `Incorrect newline style`. Já está
corrigido localmente (`core.autocrlf=false`, `core.eol=lf`), e os arquivos foram
normalizados para LF.

**Armadilha que custou um CI vermelho:** eu "conferi" os fins de linha com

```powershell
git show HEAD:scripts/x.sh | Out-File tmp   # ERRADO
```

O `Out-File` do PowerShell **acrescenta CRLF**, então eu estava medindo o meu
próprio redirecionamento, não o blob. Commitei CRLF num `.sh` e o CI quebrou
com `SC1017` e exit 127 (shebang virou `bash\r`).

**Meça sempre nos bytes:**

```powershell
$b = [System.IO.File]::ReadAllBytes((Resolve-Path "scripts/x.sh"))
# conte 10 com 13 anterior => CRLF
```

### 2.5 `giochi-engine-3d` NÃO compila nesta máquina

Ela depende de `sdl2` com a feature `bundled`, que compila SDL2 a partir do
código C — precisa de compilador C e cmake. Consequência: **não dá para
verificar mudanças nela localmente**. Foi assim que a correção do glam/glow foi
enviada com base num *probe* isolado mais o CI.

### 2.6 Três detalhes de ferramenta

- **`git commit -m` com aspas internas quebra no PowerShell**: ele passa as
  palavras como *pathspec*. Use arquivo:
  `[System.IO.File]::WriteAllText($f,$msg,(New-Object System.Text.UTF8Encoding($false)))`
  e depois `git commit -F $f`.
- **O git escreve progresso em stderr**, então o PowerShell mostra
  `[exit code: 1]` mesmo quando deu certo. Confirme com
  `git rev-parse HEAD` contra `git ls-remote origin refs/heads/main`.
- **O `Out-File -Encoding utf8` do PowerShell 5.1 põe BOM** e isso vazou para
  uma mensagem de commit (`﻿fix: ...`). Use o método do item acima.

### 2.7 Rodar a suíte do **Giochi system** nesta máquina

Não é Rust, mas é o outro repositório que você provavelmente vai precisar rodar,
e ele exige três coisas no PATH — descobrir isso custou três execuções de 10 min:

```powershell
$nodeDir = "C:\Users\Jussara dias\.dsh\dsh-runtimes\dsh-primary-runtime\dependencies\node\bin"
$pyDir   = "C:\Users\Jussara dias\.dsh\dsh-runtimes\dsh-primary-runtime\dependencies\python"
$env:PATH = "$nodeDir;$pyDir;C:\Program Files\Git\usr\bin;C:\Program Files\Git\bin;$env:PATH"
Set-Location "..\Giochi system"
& "$nodeDir\node.exe" tests/run-all.mjs
```

- **`node` no PATH** — há testes que fazem `execFileSync("node", ...)` para rodar
  um script e conferir a saída.
- **`sh` no PATH** (vem do `usr\bin` do Git) — sem ele: `spawnSync sh ENOENT`.
- **Python no PATH** — e os testes resolvem `python3` → `python`, porque no
  Windows o nome costuma ser `python` e o `python3` do PATH é o atalho da
  Microsoft Store, que responde "Python não foi encontrado".

Medido em 2026-11-11: **539 testes executam, 494 passam, 22 falham**. Antes do
commit `c541d77` do `Giochi system`, **nenhum** rodava: 16 usos de
`URL.pathname` derrubavam a suíte no import.

As 22 falhas são de ambiente. A maior família confere bit de execução com
`statSync(...).mode & 0o111`, que no Windows é sempre 0 — o título do teste diz
"no índice do Git" e o código lê do sistema de arquivos; no CI Linux os dois
coincidem.

**Efeito colateral a vigiar:** quando a suíte roda, testes que montam caminho com
`\` sem querer deixam arquivos de lixo na **raiz do repositório**, com nomes
deformados (`CUsersJUSSAR~1AppData...uptime`). Se aparecerem, não commite:
`git status` mostra eles como untracked.
### 2.8 Comandos de verificação (o que deve dar verde)
```powershell
$cargo = "$env:USERPROFILE\.cargo\bin\cargo.exe"
$env:PATH = "$env:USERPROFILE\.cargo\bin;C:\Windows\system32;C:\Windows"
Set-Location "C:\Users\Jussara dias\Desktop\plataforma sistema\Giochi engine 3d"

& $cargo fmt --all -- --check                                                  # 0
& $cargo "+stable-x86_64-pc-windows-gnu" clippy -p giochi-net --all-targets -- -D warnings   # 0
& $cargo "+stable-x86_64-pc-windows-gnu" test -p giochi-net                    # 34 passed
& $cargo "+stable-x86_64-pc-windows-gnu" test -p giochi-net --features laminar # 38 passed
& $cargo "+stable-x86_64-pc-windows-gnu" clippy -p giochi-net --features laminar --all-targets -- -D warnings  # 0
```

Repare no `+stable-x86_64-pc-windows-gnu` em tudo que **linka** (`test`,
`clippy`). `fmt` e `check` funcionam no toolchain padrão.

---

## 3. Bugs reais que os testes acharam (não repita)

### 3.1 Tamanho do inimigo no datagrama

O truncamento do snapshot usava **5 bytes** por inimigo. O codificador escreve
**9**: `id`(2) + `x`(2) + `z`(2) + `hp`(1) + `tipo`(1) + `fase_chefe`(1).

Com 150 inimigos o pacote passava de `DATAGRAMA_MAX` (1200). Datagrama grande
**fragmenta**, e perder um fragmento é perder o snapshot **inteiro**.

Corrigido com a constante `TAM_INIMIGO` e teste que monta 150 inimigos.
**Se você mudar o layout de `SnapshotInimigo`, atualize `TAM_INIMIGO` ou esse
teste vai pegar — e é bom que pegue.**

### 3.2 `0.0.0.0` não é endereço de destino

Host e cliente abrem em `0.0.0.0`, e `local()` devolvia `0.0.0.0:porta`.
`send_to` para isso falha com `AddrNotAvailable`. **Oito testes morriam nisso.**

Não era bug de teste: a API devolvia um endereço **inutilizável**. Um cliente
real receberia `0.0.0.0` do host e nunca conectaria.

O que existe agora:

- `Host::escutar(ender)` — prende a uma interface (os testes usam `127.0.0.1`);
- `Host::endereco_anunciado()` — descobre o IP da interface de saída com o
  truque do UDP `connect` (não envia pacote, só escolhe a rota);
- documentação em `Net::local` e `Host::endereco` avisando do curinga.

**Se você acrescentar um `Cliente::endereco()` para exibir na tela, use o mesmo
truque** — o curinga não serve para conectar.

### 3.3 O teste que estava errado

`aoi::a_contagem_do_relatorio_se_sustenta` usava grade de espaçamento 100 com
raio 70 e exigia 3 inimigos. Geometricamente só o ponto central cabia: a
expectativa era **impossível**. Reescrito com espaçamento 26 e raio 100.

Lição: quando um teste falha, a primeira pergunta é "a expectativa está certa?".

---

## 4. O QUE FALTA — em ordem de valor

### 4.1 ABI C (o mais importante)

**Por que é o próximo passo:** sem ela, `giochi-net` não é alcançável de C nem
do GIOCHI-OS. Hoje é uma biblioteca Rust que ninguém consegue chamar.

O relatório pede estas funções, seguindo o padrão do crate
`giochi-engine-capi` (handles opacos `u64`, retorno `int32_t`, 0 = sucesso,
negativo = erro):

```
net_host_create(porta)                          -> u64
net_client_join(endereco)                       -> u64
net_send_input(handle, input, camera_x, camera_z) -> i32
net_set_camera_pos(handle, x, z)                -> i32
net_poll_snapshot(handle, out_snapshot)         -> i32
net_poll_snapshot_filtered(handle, out, max)    -> i32
```

**O problema de projeto que você vai encontrar primeiro:** C não tem `Vec`.
`Mensagem::Inimigos` carrega `Vec<SnapshotInimigo>`, então a struct de saída
precisa de **capacidade fixa**. Sugestão:

```c
#define GIOCHI_NET_MAX_INIMIGOS 64

typedef struct {
    uint16_t id;
    float    x;
    float    z;
    uint16_t hp;
    uint8_t  tipo;
    uint8_t  fase_chefe;
} GiochiNetInimigo;

typedef struct {
    uint32_t tick;
    uint32_t total;                              /* quantos cabiam na AOI   */
    uint32_t escritos;                           /* quantos couberam aqui   */
    GiochiNetInimigo inimigos[GIOCHI_NET_MAX_INIMIGOS];
} GiochiNetSnapshot;
```

`total > escritos` é o sinal de truncamento — **o consumidor precisa saber que
perdeu inimigo**, senão desenha menos e não entende por quê.

**Passos:**

1. Ler `giochi-engine-capi` inteiro antes de escrever: há um registry de
   handles e um teste de layout de ABI (`tests/abi_layout.rs`) que **falha de
   propósito** quando a ABI muda. Ele é a rede de segurança, não um estorvo.
2. Criar `giochi-engine-capi/src/net.rs` com as funções e as structs
   `#[repr(C)]`.
3. Rodar `cbindgen` para regenerar o header (há `scripts/gen-header.sh`).
4. Atualizar `docs/C_ABI_CONTRACT.md` — **atenção**: a linha 41 hoje diz que
   *networking* está fora do escopo. Isso precisa mudar, e o relatório do
   consultor cita essa linha.
5. Atualizar o teste de layout.
6. Verificar: `cargo test -p giochi-engine-capi` e o passo "C header is up to
   date" do CI (que só falha no CI, porque compara o header regerado).

**Não faça isso sem compilador.** A sessão anterior adiou justamente por isso, e
fez bem: header gerado à mão que não corresponde ao código é pior que header
ausente.

### 4.2 Teste com dois processos

O relatório pede "dois processos locais (host e client) trocando snapshots via
UDP". Hoje os testes de `host.rs` e `client.rs` usam **sockets reais**, mas no
mesmo processo.

O que falta de verdade é pouco: um binário de teste que suba host e cliente
separados (`cargo test` não serve bem porque quer asserts no mesmo processo).
Sugestão: `giochi-net/examples/eco.rs` com `--host` / `--client`, e um teste
`#[ignore]` que faz `Command::new` e checa a saída. Marque `#[ignore]` para não
travar CI sem rede.

### 4.3 Decisão sobre mDNS

O relatório pede descoberta automática em LAN; hoje o cliente precisa do IP na
mão. `mdns-sd` não entrou porque puxa `ifaddrs`/`socket2` e **não havia
compilador para provar que cross-compila**.

**Agora há.** O caminho:

1. `cargo add mdns-sd --optional` em `giochi-net`, atrás da feature `mdns`.
2. `cargo "+stable-x86_64-pc-windows-gnu" check -p giochi-net --features mdns`.
3. `scripts/check-aarch64.sh` já roda uma verificação separada para features
   opcionais — acrescente a sua ali (foi assim que o `laminar` passou a ser
   cross-verificado).
4. Se falhar em aarch64, o substituto é **broadcast UDP** num endereço fixo:
   ~30 linhas, sem dependência, funciona em LAN plana.

### 4.4 Permissão de rede no manifesto (dois repositórios)

**O relatório afirma que "o formato de manifesto já existe no `assemble.mjs` do
GIOCHI-OS". Isso é falso** — verificado:

- `Giochi os/scripts/assemble.mjs` lê `apps.json` + `metadata.json` e empacota.
  Não há campo de permissão, nem porta UDP, nem conceito de manifesto.
- `Giochi app/schemas/metadata.schema.json` é `additionalProperties: false` e
  não tem `network`.

Então declarar rede exige mexer em **três lugares**: schema + validador no
`Giochi app`, empacotador no `Giochi os`, e o sistema (que é quem poderia abrir
porta no firewall).

**Estado atual, que ninguém decidiu:** um app pode abrir porta porque o aparelho
**não tem firewall**. Isso não é permissão, é ausência de controle.

### 4.5 Dívida do repositório que não é sua, mas você vai encontrar

- O CI tem `cancel-in-progress: true`. Empurrar para `main` cancela builds em
  andamento — em outro repositório isso já custou 1–3 h de build.
- Os `.sh` do repo são **LF**. Confira antes de commitar (§2.4).
- `docs/logo.jpg` está duplicado/untracked em outros dois repositórios desta
  família. Não é problema deste.

---

## 5. Armadilhas específicas deste código

### 5.1 `laminar`: `send` só enfileira

`Socket::send` **enfileira**; quem transmite é `manual_poll`, que roda dentro do
nosso `drenar`. Consequência: **os dois lados precisam chamar `drenar` a cada
frame**, inclusive quem só envia.

Dois testes falharam por isso, e o sintoma era "a rede não funciona" **sem erro
nenhum**. Se você escrever um teste novo com laminar e ele não entregar nada,
suspeite disto antes de suspeitar do protocolo.

### 5.2 glam: `opengl` vs `vulkan` não dá erro de compilação

`glam::camera::rh::proj::{opengl,vulkan,directx}` têm a **mesma assinatura**. Só
o `opengl` mapeia profundidade para `-1..1`, que é o que o GLES usa.

Escolher `vulkan` **compila** e produz uma cena que parece plausível até a
geometria chegar perto do plano próximo. O teste
`tests/math3d_integration.rs::the_depth_range_is_the_opengl_one_not_the_vulkan_one`
é o que prende isso — **não o remova**.

Em glam 0.31 os nomes eram `Mat4::perspective_rh_gl` (-1..1) e
`Mat4::perspective_rh` (0..1). Em 0.34 tudo virou submódulo de `glam::camera`.

### 5.3 glow 0.18: `None` mudou de significado

- `PixelPackData::Slice(&mut [u8])` → `Slice(Some(&mut [u8]))`
- `tex_image_2d(..., None)` → `PixelUnpackData::Slice(None)`

O `None` antigo, no 0.18, significa **"use o PBO ligado a `PACK_BUFFER`"** — que
é outra operação. Compilar com o `None` errado não dá erro: **lê para o lugar
errado em silêncio**.

### 5.4 O protocolo é versionado por um único número

`VERSAO_PROTOCOLO` (hoje `1`) é checado **antes** de qualquer campo ser lido. Se
você mudar o layout de qualquer variante em `wire.rs`, **suba a versão**. Um
cliente antigo falando com host novo produziria leitura deslocada — pior que
recusar.

Há teste prendendo `VERSAO_PROTOCOLO == 1` para forçar a decisão a ser
consciente.

### 5.5 O transporte padrão tem zero dependência — preserve

`giochi-net` sem features **não tem dependência nenhuma**. Isso é requisito de
cross-compile para aarch64 e foi decisão deliberada (a serialização foi escrita
à mão em vez de usar `postcard`/`bincode` por causa disso).

Se você precisar de um crate novo, **coloque atrás de feature**. O CI já tem um
passo separado para cross-verificar features opcionais em aarch64
(`scripts/check-aarch64.sh`); use-o.

---

## 6. O que NÃO está verificado (honestidade acima de otimismo)

| Item | Situação |
| --- | --- |
| `giochi-engine-3d` compilar | **não verificado localmente** (sem C/cmake). Provado pelo CI verde e por um probe de API. |
| Dois processos trocando pacotes | **não feito** — só mesmo processo |
| mDNS | não implementado |
| ABI C | não implementada |
| Comportamento em rede real (Wi-Fi, perda, MTU) | **nunca testado**. Todos os testes usam loopback. |
| Desempenho em ARM64 | **nunca medido**. Os números de AOI em `docs/REDE.md` são aritmética, não medição. |
| Hibernação/pausa do aparelho durante partida | não considerado |

**Nada disto rodou em hardware.** O primeiro teste em aparelho de verdade é o
que vai dizer se as escolhas (15 Hz de snapshot, 1200 B de datagrama, 8
clientes) se sustentam.

---

## 7. Ordem sugerida de trabalho

1. **§4.1 ABI C** — é o que torna tudo alcançável.
2. **§4.2 dois processos** — barato, e prova o que o relatório pede.
3. **§4.3 mDNS** — agora dá para verificar.
4. **§4.4 permissão de rede** — cruza repositórios; precisa de decisão de
   produto, não só de código.
5. Rodar tudo em hardware.

---

## 8. Documentos irmãos

- `docs/REDE.md` — as decisões e as comparações (por que à mão em vez de
  `postcard`, por que o raio da AOI é do jogo, tabela dos dois transportes).
- `docs/C_ABI_CONTRACT.md` — o contrato da ABI. **A linha 41 precisa mudar**
  quando a ABI de rede existir.
- `Relatório consultor externo.txt` — o pedido original (relatório 2, rede P2P).
- `docs/DECISIONS.md` — ADRs; a ADR-038 explica a convenção de profundidade.
