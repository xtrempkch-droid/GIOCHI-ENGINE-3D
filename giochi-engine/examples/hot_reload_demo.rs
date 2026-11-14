//! `hot_reload_demo` — edit a texture while the game runs.
//!
//! Creates `target/hot-reload-demo/` with two images, draws them, and watches the
//! directory. Saving either file from an editor replaces the image **on the next frame**
//! without restarting the program and without a new handle.
//!
//! The demo can also demonstrate itself: on every 40th frame it rewrites `swatch.bmp`
//! with the next colour in a cycle, which is exactly the same code path an editor save
//! takes. That is what makes it runnable in CI and on a console with no editor.
//!
//! ```text
//! cargo run -p giochi-engine --example hot_reload_demo --features hot_reload
//! cargo run -p giochi-engine --example hot_reload_demo --features hot_reload -- --frames 240
//! cargo run -p giochi-engine --example hot_reload_demo --features hot_reload -- --no-writer
//! GIOCHI_HOT_RELOAD=0 cargo run -p giochi-engine --example hot_reload_demo --features hot_reload
//! ```
//!
//! With `--no-writer` the demo only reacts to files you change yourself, which is the
//! interesting way to run it: leave it on screen and save `swatch.bmp` from any editor.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use giochi_engine::hot_reload::HotReloadConfig;
use giochi_engine::input::keyboard::KEY_ESCAPE;
use giochi_engine::{
    AudioSystem, Color, Engine, EngineConfig, Input, Rect, RenderContext, Scene, SceneTransition,
    Time,
};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;
/// Where the demo writes its editable assets.
const DEMO_DIR: &str = "target/hot-reload-demo";
const SWATCH: &str = "swatch.bmp";
const TILE: &str = "tile.bmp";
const SWATCH_SIZE: u32 = 32;
const TILE_SIZE: u32 = 64;
/// How often the automatic writer changes the swatch, in frames.
const WRITE_EVERY: u64 = 40;
/// Colours the writer cycles through: unmistakably different from each other.
const CYCLE: [(u8, u8, u8); 4] = [(220, 40, 60), (40, 200, 90), (60, 120, 240), (240, 200, 40)];

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let options = Options::parse(std::env::args().skip(1))?;
    let dir = PathBuf::from(DEMO_DIR);
    std::fs::create_dir_all(&dir).map_err(|error| format!("não criei {DEMO_DIR}: {error}"))?;

    let swatch_path = dir.join(SWATCH);
    write_swatch(&swatch_path, CYCLE[0])?;
    copy_tile(&dir.join(TILE))?;

    let mut engine = Engine::new(EngineConfig::new(WIDTH, HEIGHT, "giochi — hot_reload_demo"))
        .map_err(|error| error.to_string())?;

    // The config comes from the environment: on in a debug build, off in release, and
    // `GIOCHI_HOT_RELOAD=0` overrides both.
    let watched = engine
        .enable_hot_reload(std::slice::from_ref(&dir))
        .map_err(|error| error.to_string())?;
    if watched == 0 {
        println!(
            "hot reload desligado ({}); o demo roda, mas nada recarrega",
            if HotReloadConfig::from_env().enabled {
                "nenhum caminho vigiado"
            } else {
                "GIOCHI_HOT_RELOAD=0 ou build release"
            }
        );
    } else {
        println!(
            "hot reload ativo: edite {}/{} ou {}/{} e veja o quadro mudar",
            DEMO_DIR, SWATCH, DEMO_DIR, TILE
        );
    }

    let scene = DemoScene::new(&swatch_path, &dir.join(TILE), options.writer)?;
    if let Some(limit) = options.frames {
        engine.set_frame_limit(limit);
    }

    engine
        .run(Box::new(scene))
        .map_err(|error| error.to_string())?;

    if let Some(reloader) = engine.hot_reload() {
        let stats = reloader.stats();
        println!(
            "eventos: {} | aplicados: {} | falhas: {} | ignorados: {}",
            stats.events, stats.applied, stats.failed, stats.ignored
        );
        if stats.applied == 0 {
            println!(
                "nenhum recarregamento: rode sem --no-writer ou salve {DEMO_DIR}/{SWATCH} \
                 enquanto o demo está aberto"
            );
        }
    }
    Ok(())
}

/// Command line, parsed by hand like the other examples.
struct Options {
    frames: Option<u64>,
    /// Whether the demo rewrites the swatch by itself.
    writer: bool,
}

impl Options {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Options {
            frames: None,
            writer: true,
        };
        let mut arguments = arguments.peekable();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--no-writer" => options.writer = false,
                "--frames" => {
                    let value = arguments
                        .next()
                        .ok_or_else(|| "--frames precisa de um valor".to_owned())?;
                    let frames: u64 = value
                        .parse()
                        .map_err(|_| format!("--frames inválido: {value}"))?;
                    if frames == 0 {
                        return Err("--frames 0 não desenha nada".to_owned());
                    }
                    options.frames = Some(frames);
                }
                other => return Err(format!("argumento desconhecido: {other}")),
            }
        }
        Ok(options)
    }
}

/// The engine scene: two sprites and a small HUD made of rectangles.
struct DemoScene {
    swatch: PathBuf,
    tile: PathBuf,
    swatch_texture: u64,
    tile_texture: u64,
    writer: bool,
    colour_index: usize,
    frame: u64,
    quit: bool,
}

impl DemoScene {
    fn new(swatch: &Path, tile: &Path, writer: bool) -> Result<Self, String> {
        Ok(Self {
            swatch: swatch.to_path_buf(),
            tile: tile.to_path_buf(),
            swatch_texture: 0,
            tile_texture: 0,
            writer,
            colour_index: 0,
            frame: 0,
            quit: false,
        })
    }

    /// Rewrites the swatch with the next colour in the cycle.
    fn advance_swatch(&mut self) {
        self.colour_index = (self.colour_index + 1) % CYCLE.len();
        if let Err(error) = write_swatch(&self.swatch, CYCLE[self.colour_index]) {
            eprintln!("erro ao escrever {}: {error}", self.swatch.display());
        }
    }
}

impl Scene for DemoScene {
    fn on_enter(&mut self) {
        // The scene creates its own textures, so it has to undo that in `on_exit`. A
        // reload keeps these handles alive, which is the property being demonstrated.
        println!("on_enter: as texturas são criadas no primeiro render e nunca trocam de handle");
    }

    fn on_exit(&mut self) {
        println!(
            "on_exit: as duas texturas viveram a execução inteira \
             (handles {swatch} e {tile})",
            swatch = self.swatch_texture,
            tile = self.tile_texture
        );
    }

    fn update(&mut self, _time: Time, input: &Input, _audio: &mut AudioSystem) {
        if input.key_pressed(KEY_ESCAPE) {
            self.quit = true;
        }
    }

    fn transition(&mut self) -> SceneTransition {
        if self.quit {
            SceneTransition::Quit
        } else {
            SceneTransition::Continue
        }
    }

    fn render(&mut self, render: &mut RenderContext) {
        // Textures are created lazily here because `on_enter` has no renderer.
        if self.swatch_texture == 0 {
            match render.create_texture(&self.swatch.to_string_lossy()) {
                Ok(handle) => self.swatch_texture = handle,
                Err(error) => {
                    eprintln!("erro ao carregar {}: {error}", self.swatch.display());
                    return;
                }
            }
            match render.create_texture(&self.tile.to_string_lossy()) {
                Ok(handle) => self.tile_texture = handle,
                Err(error) => {
                    eprintln!("erro ao carregar {}: {error}", self.tile.display());
                    return;
                }
            }
        }

        if self.writer && self.frame % WRITE_EVERY == 0 && self.frame > 0 {
            self.advance_swatch();
        }
        self.frame += 1;

        // The swatch, blown up so a one-pixel change is visible.
        let swatch_size = 240.0;
        if let Err(error) = render.draw_texture(
            self.swatch_texture,
            Rect::new(80.0, 150.0, swatch_size, swatch_size),
            Rect::ZERO,
        ) {
            eprintln!("erro ao desenhar o swatch: {error}");
        }

        // The tile, tiled across the right half.
        for row in 0..3 {
            for column in 0..3 {
                if let Err(error) = render.draw_texture(
                    self.tile_texture,
                    Rect::new(
                        460.0 + column as f32 * (TILE_SIZE as f32 + 8.0),
                        150.0 + row as f32 * (TILE_SIZE as f32 + 8.0),
                        TILE_SIZE as f32,
                        TILE_SIZE as f32,
                    ),
                    Rect::ZERO,
                ) {
                    eprintln!("erro ao desenhar o tile: {error}");
                }
            }
        }

        // A frame counter bar: it moves, so "the program is alive" is never in doubt
        // while you edit a file.
        let width = render.size().0 as f32;
        let phase = (self.frame as f32 * 0.02).sin() * 0.5 + 0.5;
        render.draw_rect(
            Rect::new(0.0, HEIGHT as f32 - 12.0, width * phase, 12.0),
            Color::rgb(90, 140, 220),
        );
    }
}

/// Writes a `width`x`width` 24-bit BMP filled with `color`.
fn write_bmp(path: &Path, width: u32, height: u32, color: (u8, u8, u8)) -> Result<(), String> {
    let row_bytes = (width * 3).div_ceil(4) * 4;
    let pixel_bytes = row_bytes * height;
    let file_size = 14 + 40 + pixel_bytes;

    let mut bytes = Vec::with_capacity(file_size as usize);
    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&file_size.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(14u32 + 40).to_le_bytes());
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&(width as i32).to_le_bytes());
    bytes.extend_from_slice(&(height as i32).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&24u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&pixel_bytes.to_le_bytes());
    bytes.extend_from_slice(&2835i32.to_le_bytes());
    bytes.extend_from_slice(&2835i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());

    let padding = row_bytes - width * 3;
    for _ in 0..height {
        for _ in 0..width {
            bytes.extend_from_slice(&[color.2, color.1, color.0]);
        }
        bytes.extend(std::iter::repeat_n(0u8, padding as usize));
    }

    std::fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn write_swatch(path: &Path, color: (u8, u8, u8)) -> Result<(), String> {
    write_bmp(path, SWATCH_SIZE, SWATCH_SIZE, color)
}

/// Copies `assets/checker.bmp` next to the swatch, so the demo has one file the user can
/// replace with any image and one the writer never touches.
fn copy_tile(destination: &Path) -> Result<(), String> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/checker.bmp");
    let bytes = std::fs::read(&source)
        .map_err(|error| format!("não li '{}': {error}", source.display()))?;
    std::fs::write(destination, bytes)
        .map_err(|error| format!("não escrevi '{}': {error}", destination.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bmp_header_is_the_size_the_reader_expects() {
        let path = std::env::temp_dir().join(format!("giochi-demo-bmp-{}.bmp", std::process::id()));
        write_bmp(&path, 4, 4, (1, 2, 3)).expect("write");
        let bytes = std::fs::read(&path).expect("read");

        assert_eq!(&bytes[..2], b"BM");
        assert_eq!(
            bytes.len(),
            14 + 40 + 16 * 3,
            "4x4 rows are 12 bytes, padded to 12"
        );
        // The pixel data starts at the documented offset.
        assert_eq!(&bytes[10..14], &(54u32).to_le_bytes());
        // Bottom-up BGR.
        assert_eq!(&bytes[54..57], &[3, 2, 1]);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn options_reject_unknown_arguments() {
        let error = Options::parse(["--wat".to_owned()].into_iter()).expect_err("unknown");
        assert!(error.contains("desconhecido"));
    }

    #[test]
    fn options_parse_the_frame_limit() {
        let options =
            Options::parse(["--frames".to_owned(), "10".to_owned()].into_iter()).expect("valid");
        assert_eq!(options.frames, Some(10));
        assert!(options.writer);
    }

    #[test]
    fn a_zero_frame_limit_is_refused() {
        let error =
            Options::parse(["--frames".to_owned(), "0".to_owned()].into_iter()).expect_err("zero");
        assert!(error.contains("0"));
    }
}
