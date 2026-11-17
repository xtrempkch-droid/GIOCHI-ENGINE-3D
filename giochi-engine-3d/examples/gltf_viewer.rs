//! Loads a glTF 2.0 (`.gltf` or `.glb`) and shows it spinning, one draw per primitive.
//!
//! ```text
//! cargo run -p giochi-engine-3d --example gltf_viewer -- modelo.gltf
//! cargo run -p giochi-engine-3d --example gltf_viewer -- carro.glb --frames 3
//! ```
//!
//! # What this demonstrates that `model_viewer` does not
//!
//! A glTF file is a *scene*: several meshes, each with its own material, arranged by a tree of
//! nodes that carry transforms. [`RenderContext3D::load_gltf_meshes`] uploads one mesh per
//! primitive and the node transforms are already baked into the vertices, so this example
//! draws a list of pieces — each with its own colour, emissive, metalness and roughness — and
//! spins them together by using the same model matrix for all of them.
//!
//! The camera **frames the model** from the bounds the loader computed, because a downloaded
//! asset is never at a convenient scale: a car is four metres long, a bolt is four
//! millimetres, and both should fill the window.
//!
//! # What it does not do
//!
//! Textures. The loader reads material *factors*, not images: a model whose colour comes from
//! a PNG shows its base colour factor instead. Vertex colours are likewise ignored, and there
//! is no skinning (the engine draws static geometry — see `model_viewer`'s note).

use std::process::ExitCode;

use giochi_engine::platform::sdl::{quit_signal, QuitSignal};
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine::platform::Platform;
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Quat, Transform3D, Vec3};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;

/// How fast the model spins, in radians per second.
const SPIN_RATE: f32 = 0.5;
/// Extra room around the model, so it does not touch the frame edge.
const FRAMING_MARGIN: f32 = 1.2;

#[derive(Debug)]
struct Options {
    model: String,
    frames: Option<u64>,
}

impl Options {
    fn parse() -> Result<Self, String> {
        let mut args = std::env::args().skip(1);
        let mut model = None;
        let mut frames = None;
        while let Some(arg) = args.next() {
            if arg == "--help" || arg == "-h" {
                return Err(String::new());
            } else if let Some(value) = arg.strip_prefix("--frames=") {
                frames = Some(parse_number(value, "--frames")?);
            } else if arg == "--frames" {
                frames = Some(parse_number(
                    next_value(&mut args, "--frames")?,
                    "--frames",
                )?);
            } else if arg.starts_with('-') {
                return Err(format!("unknown argument '{arg}'"));
            } else if model.replace(arg).is_some() {
                return Err("only one model at a time".to_owned());
            }
        }
        Ok(Self {
            model: model.ok_or_else(|| "a .gltf or .glb path is required".to_owned())?,
            frames,
        })
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn parse_number<T: std::str::FromStr>(value: impl AsRef<str>, flag: &str) -> Result<T, String> {
    let value = value.as_ref();
    value
        .parse()
        .map_err(|_| format!("{flag} expects a number, got '{value}'"))
}

const USAGE: &str = "\
gltf_viewer — draw a glTF 2.0 model

Usage:
  gltf_viewer MODEL.gltf [--frames N]

Options:
  --frames N   render N frames and exit (headless checks)
  -h, --help   show this help
";

fn main() -> ExitCode {
    env_logger::init();
    match Options::parse()
        .map_err(Box::<dyn std::error::Error>::from)
        .and_then(run)
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.to_string().is_empty() => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let platform = Platform::new()?;
    println!("video driver: {}", platform.video_driver_name());
    context::configure_attributes(&platform, GlesVersion::from_env())?;
    let window = Window::new_opengl(&platform, WindowConfig::new(1280, 720), "gltf")?;
    let mut render = RenderContext3D::new(context::create(&window)?);
    println!(
        "GLES {} on {}",
        render.gles_version().major(),
        render.adapter()
    );

    let loaded = render.load_gltf_meshes(std::path::Path::new(&options.model))?;
    let (min, max) = loaded
        .bounds()
        .ok_or("the model has no bounds, so the camera cannot frame it")?;
    let centre = Vec3::new(
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    );
    let radius = Vec3::new(max[0] - min[0], max[1] - min[1], max[2] - min[2]).length() * 0.5;
    println!(
        "{}: {} primitive(s), {} vertices, {} triangles, radius {radius:.3}, centre {centre:?}",
        options.model,
        loaded.mesh_count(),
        loaded.model.vertex_count(),
        loaded.model.triangle_count()
    );

    let shader = render.create_builtin_shader(BuiltinShader::Phong)?;
    render.set_clear_color(Color3D::new(0.06, 0.08, 0.13, 1.0));
    // The scene's own light: a glTF file carries no lights unless it uses an extension this
    // loader does not read, so the engine's default sun does the job.
    render.set_light(giochi_engine_3d::DirectionalLight::sun());

    let fov = std::f32::consts::FRAC_PI_3;
    let distance = radius / (fov * 0.5).tan() * FRAMING_MARGIN;
    // Look at the centre from a three-quarter view, which shows depth on the first frame.
    let centre_glam = centre.to_glam();
    let eye = Vec3::from_glam(
        centre_glam + glam::Vec3::new(distance * 0.55, distance * 0.45, distance * 0.7),
    );
    let camera = Camera3D::from_parts(eye, centre, Vec3::UP)
        .with_fov_y(fov)
        .with_clip_planes((distance * 0.001).max(0.001), distance * 100.0);

    let frame_ms = if options.frames.is_some() {
        0
    } else {
        1000 / 60
    };
    let mut frames: u64 = 0;
    let mut elapsed: f32 = 0.0;

    loop {
        let started = std::time::Instant::now();
        for event in platform.poll_events() {
            if quit_signal(&event) != QuitSignal::None {
                report(&loaded, frames);
                return Ok(());
            }
        }

        let (width, height) = window.drawable_size();
        let camera = camera.with_drawable_size(width, height);
        // Spinning about the model's own centre. A `Transform3D` is T·R·S, so a rotation
        // about `c` is T(c − R·c)·R: the translation cancels the rotation's displacement of
        // the centre. Without it an off-centre model orbits out of frame instead of turning.
        let rotation = Quat::from_axis_angle(Vec3::UP, elapsed * SPIN_RATE);
        let displaced = rotation.to_glam() * centre_glam;
        let spin = Transform3D::new(
            Vec3::from_glam(centre_glam - displaced),
            rotation,
            Vec3::new(1.0, 1.0, 1.0),
        );

        render.begin_frame(&window);
        for (handle, material) in &loaded.primitives {
            let material = material
                .as_ref()
                .map(giochi_engine_3d::mesh::GltfMaterial::to_material)
                .unwrap_or_default();
            render.draw_mesh_material(*handle, shader, &spin, &camera, &material)?;
        }

        render.end_frame(&window);

        let error = render.take_gl_error();
        if error != 0 {
            return Err(format!("GL error 0x{error:04x} on frame {frames}").into());
        }

        frames += 1;
        elapsed += frame_ms as f32 / 1000.0;
        if options.frames.is_some_and(|limit| frames >= limit) {
            report(&loaded, frames);
            return Ok(());
        }
        if frame_ms > 0 {
            let spent = started.elapsed().as_millis() as u64;
            if spent < frame_ms {
                std::thread::sleep(std::time::Duration::from_millis(frame_ms - spent));
            }
        }
    }
}

fn report(loaded: &giochi_engine_3d::mesh::GltfLoaded, frames: u64) {
    println!(
        "drawn {} frame(s): {} primitive(s), {} triangles",
        frames,
        loaded.mesh_count(),
        loaded.model.triangle_count()
    );
}
