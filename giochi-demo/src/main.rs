//! GIOCHI-ENGINE demo binary.
//!
//! Runs a small interactive scene through the [`Engine`] facade, so the demo
//! exercises the same path a real game takes: platform → window → renderer →
//! input/audio → scene stack → fixed-timestep loop.
//!
//! ```text
//! cargo run -p giochi-demo                    # interactive, ESC quits
//! cargo run -p giochi-demo -- --frames 120    # deterministic run for smoke tests
//! cargo run -p giochi-demo -- lua game.lua    # run a Lua script (phase 8)
//! giochi-demo --frames 3 --quadro out.bmp     # capture a frame as BMP
//! ```
//!
//! The scene is deliberately small: a textured sprite you can move with WASD or the
//! mouse, a bouncing companion square and a beep on `Space`. It is the living proof
//! that every module works together.

use std::process::ExitCode;

use giochi_engine::input::keyboard::{KEY_A, KEY_D, KEY_S, KEY_SPACE, KEY_W};
use giochi_engine::input::mouse::MOUSE_RIGHT;
use giochi_engine::{
    AudioSystem, Color, Engine, EngineConfig, Input, RenderContext, Scene, Sprite, Time, Vec2,
};

const WINDOW_TITLE: &str = "GIOCHI-ENGINE";
const SPRITE_SIZE: f32 = 64.0;
const PLAYER_SPEED: f32 = 320.0;

fn main() -> ExitCode {
    env_logger::init();

    let options = Options::parse();
    if let Some(arguments) = options.lua.clone() {
        return run_lua(&arguments);
    }

    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Command line options.
#[derive(Debug, Default)]
struct Options {
    frames: Option<u64>,
    verify: bool,
    /// `lua <script> [args...]`: run a script instead of the built-in demo.
    lua: Option<Vec<String>>,
    /// `--quadro <arquivo.bmp>`: capture the last rendered frame.
    ///
    /// Exists for the device: "abriu" and "desenhou" are different claims, and
    /// without a capture there is no way to tell them apart over SSH. Works with
    /// the software renderer; with GLES read-back returns the swapped buffer, so
    /// the capture is taken before `present` (see `capture`).
    quadro: Option<String>,
}

impl Options {
    fn parse() -> Self {
        let mut options = Self::default();
        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            if arg == "lua" {
                // Everything after `lua` belongs to the script, unparsed.
                options.lua = Some(args.collect());
                break;
            } else if let Some(value) = arg.strip_prefix("--frames=") {
                options.frames = value.parse().ok();
            } else if arg == "--frames" {
                options.frames = args.next().and_then(|value| value.parse().ok());
            } else if arg == "--verify" {
                options.verify = true;
            } else if arg == "--quadro" {
                options.quadro = args.next();
            } else if let Some(value) = arg.strip_prefix("--quadro=") {
                options.quadro = Some(value.to_string());
            }
        }

        options
    }
}

/// Resolves an asset path.
///
/// The first candidate is relative to the working directory, which is what an
/// installed build needs: `CARGO_MANIFEST_DIR` is a **host** path frozen at compile
/// time, so a cross-compiled binary looks for `/home/.../giochi-demo/../assets/...`
/// on the device and fails with a path that does not exist there. Measured on the
/// R35S: `failed to load texture '/home/charles/.../assets/player.png'`.
///
/// The manifest directory stays as the fallback so `cargo run` keeps working from
/// any directory.
fn asset(name: &str) -> String {
    let relative = format!("assets/{name}");
    if std::path::Path::new(&relative).exists() {
        return relative;
    }
    let from_manifest = format!("{}/../assets/{name}", env!("CARGO_MANIFEST_DIR"));
    if std::path::Path::new(&from_manifest).exists() {
        return from_manifest;
    }
    relative
}

/// Loads the first texture that this build can actually decode.
///
/// PNG and JPEG are optional at build time in SDL_image, and the console image
/// ships a `libSDL2_image` that links only `libSDL2`, `libm` and `libc` — asking
/// for `player.png` there fails with "unsupported image format". BMP needs no
/// external library, so the repository keeps a `.bmp` twin of every `.png` asset
/// and this falls back to it. `scripts/png-to-bmp.py` regenerates the twins and
/// `giochi-engine/tests/asset_formats.rs` fails CI when one drifts.
fn load_texture_any(
    engine: &mut Engine,
    candidates: &[&str],
) -> Result<u64, giochi_engine::GiochiError> {
    let mut last_error = None;

    for name in candidates {
        match engine.render_mut().load_texture(&asset(name)) {
            Ok(handle) => {
                log::debug!("textura carregada: {name}");
                return Ok(handle);
            }
            Err(error) => {
                log::debug!("textura '{name}' indisponível: {error}");
                last_error = Some(error);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        giochi_engine::GiochiError::Asset(format!("nenhum candidato para textura: {candidates:?}"))
    }))
}

fn run(options: Options) -> Result<(), String> {
    println!("giochi-engine bootstrap OK");

    let config = EngineConfig::new(1280, 720, WINDOW_TITLE);
    let mut engine = Engine::new(config).map_err(|error| error.to_string())?;

    let scene = DemoScene::new(&mut engine).map_err(|error| error.to_string())?;
    engine.push_scene(Box::new(scene));

    // `--verify` proves the whole pipeline from a script: one frame, then report.
    let frames = options
        .frames
        .or(if options.verify { Some(1) } else { None });
    if let Some(limit) = frames {
        engine.set_frame_limit(limit);
    }

    if let Some(caminho) = options.quadro.as_deref() {
        // `run_current` presenteia cada quadro; a captura precisa do buffer antes
        // do swap, então o laço é manual: desenha, lê, e só então segue.
        let limite = options.frames.unwrap_or(3).max(1);
        for _ in 0..limite {
            engine.step(1.0 / 60.0).map_err(|error| error.to_string())?;
        }
        let (largura, altura) = engine.render().size();
        let pixels = engine
            .render()
            .read_pixels(giochi_engine::Rect::new(
                0.0,
                0.0,
                largura as f32,
                altura as f32,
            ))
            .map_err(|error| error.to_string())?;
        escrever_bmp(caminho, largura, altura, &pixels)?;
        println!("quadro: {caminho} ({largura}x{altura})");
    } else {
        engine.run_current().map_err(|error| error.to_string())?;
    }

    let stats = engine.run_stats();
    println!(
        "encerrado: {} frames, {} passos, {:.1} fps, cenas restantes: {}",
        stats.frames,
        stats.steps,
        stats.fps(),
        engine.scenes().depth()
    );

    if options.verify {
        println!(
            "texturas vivas: {}, sons vivos: {}, áudio: {}",
            engine.render().texture_count(),
            engine.audio().sound_count(),
            if engine.audio().is_available() {
                "ativo"
            } else {
                "silencioso"
            }
        );
    }

    Ok(())
}

/// Writes an RGBA8 buffer as a 32-bit BMP, bottom-up.
///
/// BMP and not PNG on purpose: `SDL2_image` on the device image is built **without
/// libpng** — `IMG_isPNG` exists and reports "PNG images are not supported" — so a
/// PNG writer would have to carry its own deflate implementation for a capture the
/// PC converts anyway. BMP needs no library on either side.
fn escrever_bmp(caminho: &str, largura: u32, altura: u32, pixels: &[u8]) -> Result<(), String> {
    let esperado = (largura as usize) * (altura as usize) * 4;
    if pixels.len() != esperado {
        return Err(format!(
            "quadro com {} bytes; esperado {esperado} ({largura}x{altura} RGBA)",
            pixels.len()
        ));
    }

    let tamanho_pixels = esperado;
    let cabecalho = 54usize;
    let mut bmp = Vec::with_capacity(cabecalho + tamanho_pixels);

    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((cabecalho + tamanho_pixels) as u32).to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(cabecalho as u32).to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(largura as i32).to_le_bytes());
    bmp.extend_from_slice(&(altura as i32).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&32u16.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(tamanho_pixels as u32).to_le_bytes());
    bmp.extend_from_slice(&2835i32.to_le_bytes());
    bmp.extend_from_slice(&2835i32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());

    // BMP guarda de baixo para cima e em BGRA; `read_pixels` devolve RGBA de cima
    // para baixo. Trocar só a ordem dos canais deixaria a imagem de cabeça para
    // baixo — que é o mesmo sintoma de rotação errada e manda procurar no lugar
    // errado.
    for y in (0..altura as usize).rev() {
        let linha = &pixels[y * largura as usize * 4..(y + 1) * largura as usize * 4];
        for pixel in linha.chunks_exact(4) {
            bmp.push(pixel[2]);
            bmp.push(pixel[1]);
            bmp.push(pixel[0]);
            bmp.push(255);
        }
    }

    std::fs::write(caminho, &bmp).map_err(|error| format!("não escrevi {caminho}: {error}"))
}

/// Runs a Lua script: `giochi-demo lua <script.lua> [args...]`.
///
/// The first argument is the script; the rest are passed to it as `arg[1..]`, with
/// `arg[0]` set to the script path, matching the usual Lua convention. A script can
/// therefore be tested from a shell without a second entry point:
///
/// ```bash
/// cargo run -p giochi-demo -- lua giochi-engine-lua/examples/mini_game.lua --auto --frames 600
/// ```
fn run_lua(arguments: &[String]) -> ExitCode {
    let Some(script) = arguments.first() else {
        eprintln!("erro: uso: giochi-demo lua <script.lua> [argumentos...]");
        return ExitCode::FAILURE;
    };

    let lua = match giochi_engine_lua::new_vm() {
        Ok(lua) => lua,
        Err(error) => {
            eprintln!("erro: não foi possível criar a VM Lua: {error}");
            return ExitCode::FAILURE;
        }
    };

    match giochi_engine_lua::run_file(&lua, script, &arguments[1..]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro no script '{script}':\n{error}");
            ExitCode::FAILURE
        }
    }
}

/// The demo scene: a movable sprite plus a bouncing square.
struct DemoScene {
    player_sprite: Sprite,
    companion: Sprite,
    companion_velocity: Vec2,
    beep: u64,
    bounds: Vec2,
}

impl DemoScene {
    fn new(engine: &mut Engine) -> Result<Self, giochi_engine::GiochiError> {
        let player = load_texture_any(engine, &["player.png", "player.bmp"])?;

        let (width, height) = engine.render().size();
        let spawn = Vec2::new(
            width as f32 * 0.5 - SPRITE_SIZE * 0.5,
            height as f32 * 0.5 - SPRITE_SIZE * 0.5,
        );

        // The beep is optional: without an audio device the handle stays 0 and
        // `play_sound` reports the reason.
        let beep = match audio_load_beep(engine.audio_mut()) {
            Ok(handle) => handle,
            Err(error) => {
                log::warn!("som indisponível: {error}");
                0
            }
        };

        Ok(Self {
            player_sprite: Sprite::new(player, spawn.x, spawn.y, SPRITE_SIZE, SPRITE_SIZE),
            companion: Sprite::new(player, 96.0, 96.0, 48.0, 48.0),
            companion_velocity: Vec2::new(180.0, 140.0),
            beep,
            bounds: Vec2::new(width as f32, height as f32),
        })
    }
}

impl Scene for DemoScene {
    fn on_enter(&mut self) {
        log::info!("DemoScene on_enter");
    }

    fn on_exit(&mut self) {
        log::info!("DemoScene on_exit");
    }

    fn update(&mut self, time: Time, input: &Input, audio: &mut AudioSystem) {
        // Keyboard movement, normalised so diagonals are not faster.
        let mut direction = Vec2::ZERO;
        if input.key_down(KEY_A) {
            direction.x -= 1.0;
        }
        if input.key_down(KEY_D) {
            direction.x += 1.0;
        }
        if input.key_down(KEY_W) {
            direction.y -= 1.0;
        }
        if input.key_down(KEY_S) {
            direction.y += 1.0;
        }

        if direction.length() > 0.0 {
            let normalised = direction * (1.0 / direction.length());
            self.player_sprite.translate(
                normalised.x * PLAYER_SPEED * time.delta,
                normalised.y * PLAYER_SPEED * time.delta,
            );
        }

        if input.key_pressed(KEY_SPACE) && self.beep != 0 {
            // A silent engine returns an error here; the demo keeps playing.
            if let Err(error) = audio.play_sound(self.beep, 0) {
                log::debug!("sem áudio: {error}");
            }
        }

        // Holding the right button drags the sprite to the cursor.
        if input.mouse().is_down(MOUSE_RIGHT) {
            let (mouse_x, mouse_y) = input.mouse().position();
            self.player_sprite.x = mouse_x - SPRITE_SIZE * 0.5;
            self.player_sprite.y = mouse_y - SPRITE_SIZE * 0.5;
        }

        // Keep the player on screen.
        let width = self.bounds.x;
        let height = self.bounds.y;
        self.player_sprite.x = self
            .player_sprite
            .x
            .clamp(0.0, (width - SPRITE_SIZE).max(0.0));
        self.player_sprite.y = self
            .player_sprite
            .y
            .clamp(0.0, (height - SPRITE_SIZE).max(0.0));

        // The companion bounces, which shows sub-pixel motion from fixed steps.
        self.companion.x += self.companion_velocity.x * time.delta;
        self.companion.y += self.companion_velocity.y * time.delta;

        if self.companion.x < 0.0 {
            self.companion.x = 0.0;
            self.companion_velocity.x = self.companion_velocity.x.abs();
        } else if self.companion.x > width - self.companion.w {
            self.companion.x = width - self.companion.w;
            self.companion_velocity.x = -self.companion_velocity.x.abs();
        }
        if self.companion.y < 0.0 {
            self.companion.y = 0.0;
            self.companion_velocity.y = self.companion_velocity.y.abs();
        } else if self.companion.y > height - self.companion.h {
            self.companion.y = height - self.companion.h;
            self.companion_velocity.y = -self.companion_velocity.y.abs();
        }
    }

    fn render(&mut self, renderer: &mut RenderContext) {
        // The engine already cleared the screen; draw the scene contents only.
        let (width, height) = renderer.size();

        for x in (0..width).step_by(128) {
            renderer.draw_rect(
                giochi_engine::Rect::new(x as f32, 0.0, 1.0, height as f32),
                Color::rgb(26, 30, 42),
            );
        }

        let _ = renderer.draw_sprite(&self.companion);
        let _ = renderer.draw_sprite(&self.player_sprite);

        // Corner marker: changes colour when the beep handle is valid, which makes
        // "audio loaded" visible in a screenshot-free smoke run.
        renderer.draw_rect(
            giochi_engine::Rect::new(width as f32 - 24.0, height as f32 - 24.0, 16.0, 16.0),
            if self.beep != 0 {
                Color::rgb(70, 200, 120)
            } else {
                Color::rgb(120, 60, 60)
            },
        );
    }

    fn transition(&mut self) -> giochi_engine::SceneTransition {
        // The engine owns the frame limit and the quit flag; this scene never asks
        // for a transition of its own.
        giochi_engine::SceneTransition::Continue
    }
}

/// Loads the beep used by the demo, generating it when it does not exist yet.
fn audio_load_beep(audio: &mut AudioSystem) -> Result<u64, giochi_engine::GiochiError> {
    let path = std::env::temp_dir().join("giochi-demo-beep.wav");
    if !path.exists() {
        write_beep_wav(&path)?;
    }
    audio.load_sound(path.to_str().unwrap_or_default())
}

/// Writes a short 16-bit stereo WAV tone.
fn write_beep_wav(path: &std::path::Path) -> Result<(), giochi_engine::GiochiError> {
    const SAMPLE_RATE: u32 = 44_100;
    const FRAMES: u32 = SAMPLE_RATE / 8;

    let mut samples = Vec::with_capacity(FRAMES as usize * 4);
    for frame in 0..FRAMES {
        let t = frame as f32 / SAMPLE_RATE as f32;
        let value = (t * 880.0 * std::f32::consts::TAU).sin() * 0.2;
        let sample = (value * f32::from(i16::MAX)) as i16;
        samples.extend_from_slice(&sample.to_le_bytes());
        samples.extend_from_slice(&sample.to_le_bytes());
    }

    let data_len = samples.len() as u32;
    let mut wav = Vec::with_capacity(44 + samples.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&samples);

    std::fs::write(path, wav).map_err(|error| {
        giochi_engine::GiochiError::Asset(format!("failed to write beep: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_default_to_an_interactive_run() {
        let options = Options::default();

        assert_eq!(options.frames, None);
        assert!(!options.verify);
    }

    #[test]
    fn the_beep_wav_is_a_valid_riff_file() {
        let path =
            std::env::temp_dir().join(format!("giochi-demo-test-{}.wav", std::process::id()));

        write_beep_wav(&path).expect("write");
        let bytes = std::fs::read(&path).expect("read");

        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert!(bytes.len() > 44);
        let _ = std::fs::remove_file(&path);
    }
}
