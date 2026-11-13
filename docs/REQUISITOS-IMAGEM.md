# O que a imagem do sistema precisa fornecer

Este é o contrato entre a engine e a **imagem** do aparelho — não o kernel, não o
launcher: o conjunto de bibliotecas, dispositivos e serviços que a engine assume
existir. Cada linha foi medida, e traz o comando que a verifica, para poder ser
repetida noutro aparelho antes de declarar suporte.

Medido em 2026-11-13 num **R35S** (RK3326 / Cortex-A35, Mali-G31 Panfrost,
Buildroot 2024.02, glibc 2.38, SDL2 2.28.5, 640x480). Números de outros
aparelhos em [KMSDRM.md](KMSDRM.md).

## 1. Resumo

| # | Item | Estado | Se faltar |
| --- | --- | --- | --- |
| 1 | GPU GLES 2.0 + KMSDRM | ✅ | nenhuma janela abre |
| 2 | SDL2 + image/mixer/ttf | ✅ | a engine não inicia |
| 3 | Controle embutido no `/dev/input` | ✅ | jogo sem entrada |
| 4 | Áudio ALSA que abre em 48 kHz | ✅ | jogo silencioso |
| 5 | `libpng`/`libjpeg`/`libwebp` | ✅ | textura em PNG/JPEG falha |
| 6 | Fonte TTF instalada | ✅ | texto em `SDL_ttf` vazio |
| 7 | Multicast na interface de rede | ✅ | mDNS não descobre nada |
| 8 | **Gateway e DNS anunciados por DHCP que não existem** | ❌ | o PC fica sem internet "por culpa do console" |
| 9 | **Rota default no aparelho** | ❌ | qualquer saída para fora da LAN falha |
| 10 | `ldd` | ❌ | diagnóstico de biblioteca ausente vira adivinhação |
| 11 | glibc ≥ 2.39 | ❌ | `cargo test` não roda no aparelho |
| 12 | Driver de vídeo `dummy` no SDL2 | ❌ | teste automatizado precisa tomar o console |
| 13 | `/dev/dri` e `/dev/input` para um grupo, não só root | ❌ | tudo tem de rodar como root |
| 14 | Cabeçalhos e `.pc` do SDL2 na imagem | ❌ | não se compila nada no aparelho |
| 15 | Térmica com folga | ⚠️ | os números de frame time não valem depois de 10 min |

Os itens 8 a 14 são de imagem; o 15 é de hardware e só se contorna no software.

## 2. O que a imagem já entrega (medido)

### 2.1 Vídeo e GPU

```sh
ls /usr/lib/libEGL.so* /usr/lib/libGLESv2.so* /usr/lib/libgbm.so*
```

Presentes. `libGLESv2.so.2.0.0` é o Panfrost/Mesa, **GLES 2.0**.

O número de drivers de vídeo do SDL2 é a informação que decide como se testa no
aparelho:

```sh
# no aparelho, com o console livre
/tmp/probe        # SDL_GetNumVideoDrivers / SDL_GetVideoDriver
```

Resultado: **`video drivers: 1 -> KMSDRM`**. Não há `dummy` nem `offscreen`.
Consequência prática: `SDL_VIDEODRIVER=dummy` **não** funciona aqui, e um teste
que força `offscreen` reporta "skipped" e a suíte ainda diz `ok`. Os testes de
integração passaram a respeitar um `SDL_VIDEODRIVER` já definido
(`tests/common::set_headless_driver`), então rodam com `kmsdrm` no aparelho sem
mudar o CI.

### 2.2 Entrada

```sh
cat /proc/bus/input/devices | grep -E '^N:|^H:'
```

```
N: Name="rk817_int Headphones"   H: Handlers=event0
N: Name="gpio-keys-vol"          H: Handlers=kbd event1
N: Name="r36s_Gamepad"           H: Handlers=event2
```

O SDL2 vê o controle **sem `libudev`** (a imagem não tem `libudev.so`): o
backend de joystick do Linux varre `/dev/input/event*` na inicialização. Medido:

```
joysticks: 1
  [0] 'r36s_Gamepad' controller=1
opened 'r36s_Gamepad' axes=4 buttons=17
```

`controller=1` quer dizer que o SDL já tem mapeamento pronto — os botões chegam
com nome (`a`, `b`, `start`), não como números crus.

**O que isso custa:** sem `libudev` não há *hotplug*. Controle embutido funciona;
controle USB ligado depois do boot **não** aparece até a engine reabrir. Se um dia
isso importar, a imagem precisa de `libudev`.

### 2.3 Áudio

```
audio devices: 1
  [0] 'rk817_int, ff070000.i2s-rk817-hifi rk817-hifi-0'
audio opened: freq=48000 ch=2 samples=1024
```

O `S51mixer` restaura `/etc/giochi/asound.state` com `alsactl` e espera até 20 s
o card aparecer — o ASoC registra o card em *probe* assíncrono, e sem essa espera
o sintoma seria "silêncio em alguns boots". `/usr/bin/amixer` existe.

### 2.4 Imagens e fontes

```
/usr/lib/libpng16.so.16.41.0
/usr/lib/libjpeg.so.8.2.2
/usr/lib/libwebp.so.7.1.8
/usr/share/fonts/dejavu
```

PNG e JPEG funcionam (ver §8.4 de [KMSDRM.md](KMSDRM.md) para o caso em que o
`libSDL2_image` está sem codec externo: aí sobra o BMP, e a engine tem esse
caminho).

### 2.5 Rede USB e multicast

O gadget monta `usb0` com IP fixo **10.55.0.2/24** (`giochi-usbnet`, iniciado por
`S30usbnet`), e um `udhcpd` entrega **10.55.0.1** ao PC.

Multicast funciona na interface, que é o que o mDNS exige — medido nos dois
sentidos, console→PC e PC→console:

```
# console anuncia, PC descobre
found 10.55.0.2:47721
# PC anuncia com --ip 10.55.0.1, console descobre
found 10.55.0.1:47800
```

`net.ipv4.icmp_echo_ignore_broadcasts = 1` não atrapalha: bloqueia ping de
broadcast, não datagrama de multicast.

### 2.6 Relógio, entropia, watchdog, governor

```sh
cat /sys/class/rtc/rtc0/name        # rk808-rtc
ls /etc/init.d/S01seedrng           # presente
cat /proc/[0-9]*/comm | grep watchdogd
cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor   # schedutil
```

Relógio de hardware mantém a data (sem NTP, o que basta para offline); a entropia
é semeada cedo, o que o mDNS precisa para sortear o *query id*; o `watchdogd` está
armado; o governor é `schedutil`, a escolha certa para carga de jogo.

## 3. O que falta, e o que muda

### 3.1 O DHCP anuncia um gateway e um DNS que o aparelho não tem

`/etc/udhcpd.conf` é gerado por `giochi-usbnet` e diz:

```
option	router	10.55.0.2
option	dns	10.55.0.2
```

Mas, no aparelho:

```sh
ip route                 # só 10.55.0.0/24 dev usb0 — NENHUMA rota default
sysctl net.ipv4.ip_forward   # 0
command -v nft iptables      # nada instalado
```

E, do PC:

```sh
nslookup example.com 10.55.0.2
;; communications error to 10.55.0.2#53: connection refused
```

Ou seja: o console se anuncia como porteiro de uma porta que não existe. O PC
recebe uma rota default com métrica alta apontando para o console e um servidor
DNS que recusa conexão. Neste PC não quebrou nada porque a Wi-Fi tem métrica 600
e a USB 20100, mas numa máquina cujo único adaptador seja esse cabo, **a internet
inteira do PC para de funcionar e o culpado parece ser o console**.

Duas saídas, ambas de uma linha:

* **Honesta**: tirar as duas linhas `option router` e `option dns` do
  `udhcpd.conf`. O cabo passa a ser o que é — um link ponto a ponto para SSH e
  para a descoberta mDNS, sem prometer saída para a internet.
* **Útil**: manter, e implementar. Exige `dnsmasq` (DNS + DHCP num processo só,
  no lugar do `udhcpd`), `net.ipv4.ip_forward=1` e uma regra de NAT
  (`nft add table ip nat ...`), **e** um uplink de verdade — o que este aparelho
  não tem, porque não tem Wi-Fi.

Para a maioria dos aparelhos da linha, a primeira é a correta.

### 3.2 O aparelho não tem rota default

```sh
ip route      # 10.55.0.0/24 dev usb0 proto kernel scope link src 10.55.0.2
```

Nenhuma. Isso já **quebrou código nosso**: `ip_anunciavel()` usava o truque de
`connect()` UDP para um destino externo para descobrir a interface de saída, e
sem rota default o kernel devolve `ENETUNREACH`. O anúncio mDNS falhava no
console e funcionava em todo PC — o pior tipo de defeito. Corrigido (ver §4).

Requisito para a imagem, se algum dia houver uplink: uma rota default, ou por
DHCP no `usb0` (o que conflita com o `udhcpd`, que é servidor nessa interface), ou
por uma variável no `giochi-usbnet` (`GIOCHI_DEFAULT_VIA=10.55.0.1`) que o PC
precisa estar encaminhando. Sem uplink, **não adicione rota default**: uma rota
morta é pior que rota nenhuma, porque esconde o erro até o `connect()`.

### 3.3 `ldd` não existe

```sh
command -v ldd     # nada
```

`ldd` é o que responde "por que o binário não abre". Sem ele, um
`version 'GLIBC_2.39' not found` ou uma `libSDL2_image` ausente viram tentativa e
erro. Custa poucos KB (`CONFIG_LDD` / pacote `ldd` no Buildroot) e economiza
sessões inteiras. Hoje o substituto é `readelf -d` (também ausente) ou o
`LD_DEBUG=libs` do próprio loader — que existe e funciona.

### 3.4 glibc 2.38 impede `cargo test` no aparelho

O `libtest` referencia `pidfd_spawnp@GLIBC_2.39` (símbolo fraco, vindo do módulo
de processo do `std`). Com glibc 2.38 o binário de teste **aborta antes do
primeiro teste**:

```
./giochi-net-<hash>: /lib/libc.so.6: version `GLIBC_2.39' not found
```

A engine não depende disso — `hardware-check.sh` valida com **exemplos**, que
linkam só o que o aparelho oferece. Mas poder rodar a suíte no alvo é outra
categoria de confiança. Requisito, se quiserem: glibc ≥ 2.39 (Buildroot 2024.02
traz 2.38; exige subir a versão do Buildroot ou fixar o glibc).

### 3.5 SDL2 sem driver de vídeo `dummy`

Só `KMSDRM`. Consequência: todo teste que abre janela precisa **tomar o console**
— parar o launcher, esperar o DRM master ser liberado, rodar, devolver. Funciona
(é o que `hardware-check.sh` faz), e tem risco: um `Ctrl-C` no meio deixa o
aparelho sem interface até o supervisor reagir.

O driver `dummy` do SDL2 (`SDL_VIDEODRIVER=dummy`) resolve isso: janela sem
`/dev/dri`, sem DRM master, sem tocar no console. Não desenha nada de verdade, e
é exatamente o que serve para testar lógica. **Não substitui** o teste em
`kmsdrm` — os dois se complementam.

### 3.6 `/dev/dri`, `/dev/input` e `/dev/snd` são só de root

```sh
ls -l /dev/dri/     # crw------- 1 root root 226, 0 card0
ls -l /dev/input/   # crw------- 1 root root 13, 64 event0
```

Hoje funciona porque o launcher e a engine rodam como **root** — que também
significa que qualquer jogo tem acesso total ao aparelho. Se a política for um
usuário de jogo sem privilégio, a imagem precisa:

* grupo `video` (ou `render`) dono de `/dev/dri/*` com modo `0660`;
* grupo `input` dono de `/dev/input/event*`;
* grupo `audio` dono de `/dev/snd/*`;
* uma regra de `mdev`/`udev` que aplique isso no boot, já que o `devtmpfs` cria
  tudo como `0600 root:root`.

O modelo está em §3 de [KMSDRM.md](KMSDRM.md) (`crw-rw----+ root video`), mas ele
não está aplicado nesta imagem.

### 3.7 Não se compila nada no aparelho

A imagem é *runtime-only*: não há `/usr/include/SDL2`, nem `sdl2.pc`, nem
`pkg-config`. Compilar no alvo exige montar um sysroot à mão — receita em
[CROSS_COMPILE.md](CROSS_COMPILE.md). Para o fluxo normal (cross-compile no PC)
está correto e economiza dezenas de MB. Só precisa estar documentado, e está.

### 3.8 Térmica: 70 °C em repouso

```sh
cat /sys/class/thermal/thermal_zone0/temp   # 70000-73000  (soc-thermal)
cat /sys/class/thermal/thermal_zone1/temp   # 69000-71000  (gpu-thermal)
```

Os dois trips passivos estão em **70 °C** e o crítico em 115 °C; os
`cooling_device` (`cpufreq-cpu0`, `devfreq-gpu`) ficam em `cur_state=0`. Isto é
hardware, não imagem: não há ventilador. A consequência para o release é que os
números de frame time medidos (`p99 4,5 ms` no `ball_balance`) valem para uma
sessão curta em repouso térmico. Uma sessão longa vai encontrar o `devfreq` da GPU
rebaixado. Se isso for um requisito de produto, o caminho é limitar o alvo de fps
ou reduzir a carga, não mexer na imagem.

## 4. O que a engine passou a fazer por causa disto

`giochi-net/src/descobrir.rs`, `ip_anunciavel()`: quando não há rota default,
enumera as interfaces (`if-addrs`) e escolhe a primeira não-loopback que não seja
link-local, preferindo IPv4. Recusa loopback continua — um anúncio em loopback
existe no registro e **ninguém consegue conectar**. Dois testes fixam isso:
`interface_sem_rota_default_e_utilizavel` e `link_local_v6_reconhece_fe80`.
`if-addrs` entra atrás da feature `mdns`, então o build padrão do alvo continua
com **zero dependências** (`cargo tree -p giochi-net --target
aarch64-unknown-linux-gnu` mostra só o próprio crate).

O exemplo `giochi-net/examples/mdns_smoke.rs` faz o teste entre duas máquinas, e
aceita `--ip` para fixar a interface quando o PC tem várias:

```sh
# console anuncia; PC descobre
ssh root@10.55.0.2 '/tmp/mdns_smoke announce 47721 40' &
cargo run -p giochi-net --features mdns --example mdns_smoke -- discover 25

# PC anuncia na interface do console; console descobre
cargo run -p giochi-net --features mdns --example mdns_smoke -- announce 47800 35 --ip 10.55.0.1 &
ssh root@10.55.0.2 '/tmp/mdns_smoke discover 25'
```

## 5. Verificar tudo de uma vez

O `probe` do §2.1 é um `main.c` de 40 linhas; o resto é uma sessão de shell:

```sh
ssh root@<ip> '
  echo "== video ==";   ls /usr/lib/libEGL.so* /usr/lib/libGLESv2.so* /usr/lib/libgbm.so*
  echo "== entrada =="; cat /proc/bus/input/devices | grep -E "^N:|^H:"
  echo "== audio ==";   ls /dev/snd
  echo "== imagem ==";  ls /usr/lib/libpng* /usr/lib/libjpeg* /usr/lib/libwebp*
  echo "== rede ==";    ip route; sysctl net.ipv4.ip_forward
  echo "== dhcp ==";    cat /etc/udhcpd.conf 2>/dev/null | grep option
  echo "== libc ==";    /lib/libc.so.6 | head -1
  echo "== tools ==";   for t in ldd bash timeout fuser python3; do
                          printf "%s: " "$t"; command -v $t || echo MISSING; done
  echo "== permissoes =="; ls -l /dev/dri /dev/input
  echo "== termica =="; for z in /sys/class/thermal/thermal_zone*; do
                          echo "$(cat $z/type): $(( $(cat $z/temp) / 1000 ))C"; done
'
```

O que se espera hoje: tudo preenchido, `ip route` **sem** rota default,
`udhcpd.conf` com duas `option` que não se sustentam, e `ldd`/`python3`
`MISSING`.
