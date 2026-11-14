# Hot reload (Fase 19)

Editar um arquivo e ver a mudança no jogo em execução, sem reiniciar. Cobre o que
recarrega, o que **não** recarrega, por que a recarga acontece na thread principal, como
ligar e desligar, e as limitações que apareceram na implementação.

---

## 1. Ligar e desligar

```bash
# Liga (é o padrão de um build debug com a feature)
cargo run -p giochi-engine --example hot_reload_demo --features hot_reload

# Desliga sem recompilar
GIOCHI_HOT_RELOAD=0 cargo run -p giochi-engine --example hot_reload_demo --features hot_reload
```

Três chaves, em ordem de precedência:

| Chave | Efeito |
|---|---|
| `HotReloadConfig::enabled` | o que o programa pediu |
| `GIOCHI_HOT_RELOAD` | `0`/`false`/`off`/`no` desliga; `1`/`true`/`on`/`yes` liga; outro valor é ignorado com aviso |
| `cfg!(debug_assertions)` | o padrão quando ninguém disse nada: ligado em debug, desligado em release |

A feature `hot_reload` é **opt-in** em cada crate:

```toml
giochi-engine       = { features = ["hot_reload"] }   # watcher + reload de textura
giochi-engine-3d    = { features = ["hot_reload"] }   # + shader e malha
giochi-engine-lua   = { features = ["hot_reload"] }   # + reinício do script
giochi-demo         = { features = ["hot_reload"] }   # encaminha para o Lua
```

Sem a feature o módulo não existe, o `Engine` não ganha campo nenhum e o binário release
sai igual ao da Fase 18 — o gate mede isso (`cargo build --release` sem features).

No Rust:

```rust
use std::path::PathBuf;
use giochi_engine::{Engine, EngineConfig};

let mut engine = Engine::new(EngineConfig::default())?;
engine.enable_hot_reload(&[PathBuf::from("assets")])?;   // diretório: recursivo
engine.enable_hot_reload(&[PathBuf::from("game.lua")])?; // arquivo: individual
```

Chamar de novo **substitui** o conjunto vigiado; a segunda chamada é "vigie estes agora",
não "vigie também estes".

---

## 2. O que recarrega

| Extensão | O que acontece | Onde |
|---|---|---|
| `.png` `.bmp` `.jpg` `.jpeg` | a textura é **redecodificada e trocada no mesmo handle** | `RenderContext::reload_texture_path` |
| `.vert` `.frag` | o programa é recompilado; em erro, o anterior continua | `giochi_engine_3d::hot_reload::ShaderReloader` |
| `.obj` | a malha é reparseada e reenviada, mesmo handle | `RenderContext3D::reload_mesh` |
| `.lua` | a **VM reinicia**: o processo para o loop e o host roda o script de novo | `giochi_engine_lua::watch_script` |

O handle **nunca muda**. É o ponto do recurso: uma cena guarda `TextureHandle`/`ShaderHandle`
nos próprios campos, um lote pode tê-los em cache, e a C ABI os entrega como `uint64_t`.
Emitir um handle novo exigiria que todo mundo que o segura aprendesse sobre ele.

Ordem de recarga por prioridade (configurável em `AssetReloadRegistry`):

```
shader (30) → malha (25) → textura (20) → script (10)
```

O shader vem primeiro porque um erro de compilação não deve ser mascarado por uma textura
que falhou; o script vem por último porque, ao reiniciar, ele vai tocar em tudo.

---

## 3. O que **não** recarrega

Estas são as fronteiras, e elas são reais:

1. **Textura cujo tamanho mudou.** Os sprites em voo guardam o recorte de origem; uma
   imagem 64×64 trocada por 128×128 recarrega, mas quem desenha com `src = (0,0,64,64)`
   vai mostrar um quarto da imagem nova. Redimensionar é uma decisão do jogo, não do
   watcher.
2. **Malha cujo número de vértices mudou** — na verdade *essa* recarrega (o buffer é
   refeito); o que não recarrega é um **VAO externo** que aponte para os buffers antigos.
   No 3D desta engine o VAO pertence à `Mesh`, então o caso não ocorre.
3. **Qualquer coisa que o jogo copiou do asset no load**, e não apenas o handle: um
   `Vec2` de tamanho lido de `texture_size` no `on_enter` continua com o valor antigo.
4. **Código Rust.** `.rs` não é asset; recarregar código é `cargo` com outra técnica
   (`subsecond`, dylib hot-swap), fora do escopo (ver `IDEAS.md`).
5. **O próprio shader embutido (`BuiltinShader`).** Ele vem de `include_str!` — faz parte
   do binário, não do disco. Só shaders carregados por `load_shader` são vigiados.
6. **Estado do script.** Um `.lua` recarregado começa de uma VM nova: globais zeradas.
   Persistência entre recargas é um sistema de save, não hot reload.

---

## 4. Por que a recarga é na thread principal

`notify` chama o callback na thread dele. O canvas do SDL, o mixer e o registro de texturas
não são `Send` nem `Sync` — a engine é single-threaded por desenho (ver a documentação do
crate). Então a thread de watch faz a única coisa que é segura de outra thread: empurra o
caminho num canal. `HotReloader::poll` drena esse canal **dentro do frame**, onde
`&mut RenderContext` existe.

```
inotify thread ──(path, kind)──▶ mpsc ──▶ poll() no frame ──▶ reload com &mut
```

Um desenho que chamasse `render.reload_texture()` do callback precisaria de
`unsafe impl Send` em metade da engine, e ainda assim seria uma corrida de dados.

### Debounce

Toda gravação de editor gera mais de um evento (write, chmod, rename por cima). O
`FileWatcher` agrupa eventos do mesmo caminho e só reporta depois de **150 ms de silêncio**
— medido a partir do *último* evento, então um save lento continua virando uma recarga só.
`Delete` seguido de `Create` dentro da janela é tratado como `Modified`, porque é o que o
"salvar" de um editor faz.

Latência total = debounce + o próximo frame ≈ **166 ms** a 60 fps. O critério da fase é
< 200 ms.

### Caminhos absolutos (defeito encontrado e corrigido)

`notify` resolve o caminho que vigia. Um `watch_dirs(&["assets"])` recebia eventos
`/home/.../assets/x.png`, e o filtro comparava com `assets/x.png` — **nada casava**, e o
resultado era um watcher que iniciava, reportava zero eventos e parecia saudável. O
`FileWatcher` agora canonicaliza os caminhos ao instalar a vigilância, e o reload de
textura compara caminhos canonicalizados (`same_file`) porque o jogo normalmente carrega
por caminho relativo. Dois testes fixam isso:
`a_relative_directory_is_watched_as_an_absolute_one` e
`a_saved_texture_reloads_in_place_without_a_new_handle`.

---

## 5. Shaders: recompilar sem perder o que estava configurado

`RenderContext3D::reload_shader_sources` compila o programa **novo** antes de mexer no
registro. Um erro de sintaxe devolve `Error3D::Shader` com o `glGetShaderInfoLog` completo
e o programa anterior continua em uso — a tela não pisca.

Uniformes:

* os que o *draw call* escreve (`u_view_proj`, `u_model`, `u_color`) são reescritos no
  frame seguinte; não precisam de nada;
* os que o jogo escreve **uma vez** (um `u_tint` constante, um `u_bias`) seriam perdidos.
  Para eles existe captura: `Program::set_capture(true)` faz cada setter guardar o valor, e
  a recarga reaplica tudo no programa novo (`apply_retained`).

Captura vem **ligada** para shaders carregados de arquivo (`load_shader`) e **desligada**
para os embutidos, que nunca recarregam e estão no caminho quente. `set_shader_capture`
permite mudar isso por handle.

---

## 6. Scripts Lua: reiniciar, não remendar

Um script é o programa; uma textura é dado que o programa lê. Não há forma suportada de
trocar o corpo de um chunk em execução, e as closures que o corpo antigo entregou ao
`app:run` continuariam apontando para as instruções antigas. O comportamento honesto é:
detectar, parar o loop, criar uma VM nova, rodar o arquivo editado.

Fluxo:

1. o host chama `giochi_engine_lua::watch_script(caminho)`;
2. `LuaScene::transition` chama `poll_script_changes()` a cada frame;
3. quando a geração muda, a cena pede `Quit` e marca `request_script_reload()`;
4. o host lê `take_reload_request()` depois de `app:run` e decide reiniciar ou sair.

`giochi-demo lua <script>` já faz isso:

```bash
cargo run -p giochi-demo --features hot_reload -- lua giochi-engine-lua/examples/mini_game.lua
# edite o .lua e salve: a janela reinicia o script
```

**Globais não sobrevivem.** Se o script precisa de estado entre recargas, ele grava num
arquivo — que é o que um sistema de save faz.

---

## 7. Integrar com editores e IDEs

Nada a fazer: o debounce existe exatamente para isso.

* **VS Code / Neovim / Emacs** salvam com write + rename (ou write + chmod). Coberto.
* **Editores que escrevem em `.tmp` e renomeiam** produzem `Deleted`+`Created` na mesma
  janela → `Modified`. Coberto.
* **`rsync`/`scp` para o console** com `--inplace` gera `Modified`; sem `--inplace` gera
  `Created` num diretório que já estava vigiado — também coberto.
* **Um `git checkout` que troca vários arquivos** gera uma rajada; cada caminho tem sua
  própria janela e todos são reportados no mesmo `poll`.

No console (ARM64), o limite de watches é o que se quebra primeiro:

```bash
sysctl fs.inotify.max_user_watches     # 8192 num Buildroot típico
```

`watch_dirs` registra **um** watch recursivo por diretório, não um por arquivo, então o
consumo é proporcional ao número de diretórios. Se o `inotify` recusar
(`ENOSPC`), a instalação da vigilância falha com `GiochiError::Asset` e a mensagem do
sistema — em vez de ligar um watcher cego.

---

## 8. Custo quando está desligado

| Situação | Custo |
|---|---|
| feature fora do build | zero: o módulo não existe, `Engine` não tem campo |
| feature no build, `enabled = false` | um `bool` por frame; nenhum `inotify`, nenhum thread |
| feature no build, `enabled = true`, nada mudando | um `try_recv` vazio + uma comparação de `Instant` por frame |
| um evento por segundo | uma canonicalização + um decode de imagem |

É o que o gate de release mede: `cargo build -p giochi-engine --release` sem features
produz o binário da Fase 18.

---

## 9. API de referência

```rust
// giochi-engine
pub mod hot_reload {
    pub struct FileWatcher;      // notify + debounce; poll() por frame
    pub struct AssetReloadRegistry; // extensão → prioridade
    pub struct HotReloader;      // os dois juntos
    pub struct HotReloadConfig;  // enabled + debounce_ms
    pub struct HotReloadEvent;   // path + kind
    pub enum   HotReloadKind { Created, Modified, Deleted }
    pub fn script_epoch() -> u32;
}

impl Engine {
    fn enable_hot_reload(&mut self, paths: &[PathBuf]) -> Result<usize, GiochiError>;
    fn enable_hot_reload_with(&mut self, config: HotReloadConfig, paths: &[PathBuf]) -> Result<usize, GiochiError>;
    fn disable_hot_reload(&mut self);
    fn is_hot_reload_enabled(&self) -> bool;
    fn register_hot_reload_handler(&mut self, extension: &str, handler: HotReloadHandler) -> bool;
    fn poll_hot_reload(&mut self) -> usize;
}

impl RenderContext {
    fn reload_texture(&mut self, handle: TextureHandle) -> Result<(), GiochiError>;
    fn reload_texture_path(&mut self, path: &Path) -> usize;
    fn reload_cached_texture(&mut self, path: &str) -> Result<TextureHandle, GiochiError>;
    fn texture_source(&self, handle: TextureHandle) -> Option<String>;
}

// giochi-engine-3d
pub mod hot_reload { pub struct ShaderReloader; }
impl RenderContext3D {
    fn reload_shader_sources(&mut self, handle: ShaderHandle, vs: &Path, fs: &Path) -> Result3D<()>;
    fn reload_mesh(&mut self, handle: MeshHandle) -> Result3D<()>;
    fn mesh_source(&self, handle: MeshHandle) -> Option<PathBuf>;
}

// giochi-engine-lua
pub fn watch_script(path: &Path) -> Result<(), String>;
pub fn watch_script_dir(dir: &Path) -> Result<(), String>;
pub fn poll_script_changes() -> u32;
pub fn take_reload_request() -> bool;
```

### Um handler próprio

Se o formato do jogo não é nenhum dos acima, o handler roda no frame, antes do caminho
embutido, e **substitui** o comportamento daquela extensão:

```rust
engine.register_hot_reload_handler("atlas9", Box::new(|render, path| {
    // recortar de novo, recarregar o que dependia, devolver true se aplicou
    log::info!("atlas '{}' mudou", path.display());
    true
}));
```

Devolver `false` conta como falha em `HotReloadStats::failed` — é assim que um jogo percebe
que o próprio formato parou de parsear.

---

## 10. Exemplo

`giochi-engine/examples/hot_reload_demo.rs`:

* cria `target/hot-reload-demo/{swatch,tile}.bmp`;
* desenha os dois (um ampliado, o outro em mosaico) e uma barra que se move — para "o
  programa está vivo" nunca ser dúvida enquanto você edita;
* a cada 40 frames reescreve `swatch.bmp` com a próxima cor, o que exercita o caminho
  completo sem editor (é o que roda em CI e no console);
* `--no-writer` desliga o escritor e deixa só o seu save;
* `--frames N` limita a execução; no fim imprime os contadores.

```bash
cargo run -p giochi-engine --example hot_reload_demo --features hot_reload -- --frames 600
# hot reload ativo: edite target/hot-reload-demo/swatch.bmp ...
# eventos: 14 | aplicados: 14 | falhas: 0 | ignorados: 0
# on_exit: as duas texturas viveram a execução inteira (handles 1 e 2)
```
