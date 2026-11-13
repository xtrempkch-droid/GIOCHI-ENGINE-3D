//! Loads an arbitrary Wavefront OBJ and shows it spinning.
//!
//! The other 3D examples use the procedural primitives (`cube`, `sphere`, `plane`)
//! because they must run with no assets on disk. That leaves the impression that the
//! engine can only draw those, which is not true: [`RenderContext3D::load_obj_mesh`]
//! uploads any triangle mesh, and it is what a real game uses for a character, a
//! dragon, a dungeon, a weapon.
//!
//! ```text
//! cargo run -p giochi-engine-3d --example model_viewer -- modelo.obj
//! cargo run -p giochi-engine-3d --example model_viewer -- dragao.obj --texture dragao.png
//! cargo run -p giochi-engine-3d --example model_viewer -- dragao.obj --frames 3 \
//!     --quadro dragao.bmp
//! ```
//!
//! The camera **frames the model automatically**: the vertices are read back from the
//! OBJ, the bounding sphere is computed, and the camera is placed far enough that the
//! whole thing fits. Without that, every model would need a hand-tuned distance and the
//! first thing a user would see is a model too big to fit or too small to see.
//!
//! # What an OBJ must contain
//!
//! Positions and triangles. Normals and UVs are optional — the loader derives face
//! normals when `vn` is missing and uses `(0, 0)` when `vt` is missing — so an export
//! with "normals off" still shades correctly. Quads are triangulated on load.
//!
//! # What it does not support (yet)
//!
//! * Skeletal animation. The engine uploads static geometry; a rigged character animates
//!   by loading one mesh per pose, or by transforming parts separately.
//! * Materials from `.mtl`. Several `o`/`g` objects are merged into one mesh, so one
//!   draw and one texture per file. Split the model into files if parts need different
//!   images.

use std::process::ExitCode;

use giochi_engine::platform::sdl::{quit_signal, QuitSignal};
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine::platform::Platform;
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Quat, Transform3D, Vec3};
use giochi_engine_3d::mesh::{load_obj, Vertex};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;

/// How fast the model spins, in radians per second.
const SPIN_RATE: f32 = 0.6;
/// Extra room around the model's bounding sphere, so it does not touch the frame edge.
const FRAMING_MARGIN: f32 = 1.25;

#[derive(Debug)]
struct Options {
    model: String,
    frames: Option<u64>,
    texture: Option<String>,
    quadro: Option<String>,
}

impl Options {
    fn parse() -> Result<Self, String> {
        let mut args = std::env::args().skip(1);
        let mut model = None;
        let mut frames = None;
        let mut texture = None;
        let mut quadro = None;

        while let Some(arg) = args.next() {
            if let Some(value) = arg.strip_prefix("--frames=") {
                frames = Some(parse_number(value, "--frames")?);
            } else if arg == "--frames" {
                frames = Some(parse_number(
                    next_value(&mut args, "--frames")?,
                    "--frames",
                )?);
            } else if let Some(value) = arg.strip_prefix("--texture=") {
                texture = Some(value.to_string());
            } else if arg == "--texture" {
                texture = Some(next_value(&mut args, "--texture")?);
            } else if let Some(value) = arg.strip_prefix("--quadro=") {
                quadro = Some(value.to_string());
            } else if arg == "--quadro" {
                quadro = Some(next_value(&mut args, "--quadro")?);
            } else if arg.starts_with('-') {
                return Err(format!(
                    "unknown argument '{arg}'; usage: model_viewer MODEL.obj \
                     [--texture PNG] [--frames N] [--quadro BMP]"
                ));
            } else if model.replace(arg).is_some() {
                return Err("only one model at a time".to_owned());
            }
        }

        Ok(Self {
            model: model.ok_or("a .obj path is required; see --help in the source")?,
            frames,
            texture,
            quadro,
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

/// Centre and radius of the bounding sphere of `vertices`.
///
/// Uses the axis-aligned bounding box rather than a true minimal sphere: a box is exact
/// to compute in one pass and its sphere is never smaller than the mesh, which is what
/// "the model must fit" needs.
fn bounding_sphere(vertices: &[Vertex]) -> (Vec3, f32) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for vertex in vertices {
        for axis in 0..3 {
            min[axis] = min[axis].min(vertex.position[axis]);
            max[axis] = max[axis].max(vertex.position[axis]);
        }
    }

    let centre = Vec3::new(
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    );
    let extent = Vec3::new(max[0] - min[0], max[1] - min[1], max[2] - min[2]);
    let radius = extent.length() * 0.5;
    (centre, radius)
}

fn main() -> ExitCode {
    env_logger::init();

    match Options::parse()
        .map_err(Box::<dyn std::error::Error>::from)
        .and_then(run)
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erro: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let platform = Platform::new()?;
    println!("video driver: {}", platform.video_driver_name());

    let requested = GlesVersion::from_env();
    context::configure_attributes(&platform, requested)?;
    let window = Window::new_opengl(&platform, WindowConfig::new(1280, 720), "giochi 3D")?;
    let mut render = RenderContext3D::new(context::create(&window)?);

    println!(
        "GLES {} on {}",
        render.gles_version().major(),
        render.adapter()
    );

    // Read the file once for the geometry *and* for the framing. `load_obj_mesh` would
    // upload without telling us the bounds, and the camera would have to guess.
    let (mut vertices, indices) = load_obj(std::path::Path::new(&options.model))?;
    let (centre, radius) = bounding_sphere(&vertices);
    println!(
        "{}: {} vertices, {} indices, bounding sphere r={radius:.3}",
        options.model,
        vertices.len(),
        indices.len()
    );
    if !radius.is_finite() || radius <= f32::EPSILON {
        return Err(
            format!("the model has no volume (radius {radius}); check the OBJ's vertices").into(),
        );
    }

    // Move the model so its centre is at the origin, which is what the spin below
    // rotates around. Rotating in place around an off-centre model throws it out of frame.
    for vertex in &mut vertices {
        for axis in 0..3 {
            vertex.position[axis] -= [centre.x, centre.y, centre.z][axis];
        }
    }
    let mesh = render.create_mesh(&vertices, &indices)?;

    let texture = match options.texture.as_deref() {
        Some(path) => Some(render.create_texture(std::path::Path::new(path))?),
        None => None,
    };
    let builtin = if texture.is_some() {
        BuiltinShader::Textured
    } else {
        BuiltinShader::Phong
    };
    let shader = render.create_builtin_shader(builtin)?;
    println!("shader: {}", builtin.name());

    render.set_clear_color(Color3D::new(0.06, 0.08, 0.13, 1.0));

    // Place the camera on +Z far enough that a sphere of `radius` fits the vertical FOV.
    let camera_fov = std::f32::consts::FRAC_PI_3;
    let distance = radius / (camera_fov * 0.5).tan() * FRAMING_MARGIN;
    let base_camera = Camera3D::from_parts(Vec3::new(0.0, 0.0, distance), Vec3::ZERO, Vec3::UP)
        .with_fov_y(camera_fov)
        .with_clip_planes(distance * 0.001, distance * 100.0);

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
                report(&mut render, mesh, shader, texture, frames);
                return Ok(());
            }
        }

        let (width, height) = window.drawable_size();
        let camera = base_camera.with_drawable_size(width, height);

        let model =
            Transform3D::from_rotation(Quat::from_axis_angle(Vec3::UP, elapsed * SPIN_RATE));

        render.begin_frame(&window);
        match texture {
            Some(handle) => {
                render.draw_mesh_textured(mesh, shader, handle, &model, &camera, Color3D::WHITE)?;
            }
            None => {
                render.draw_mesh_colored(
                    mesh,
                    shader,
                    &model,
                    &camera,
                    Color3D::new(0.82, 0.74, 0.62, 1.0),
                )?;
            }
        }

        let capture = options
            .quadro
            .as_deref()
            .filter(|_| options.frames.is_some_and(|limit| frames + 1 >= limit));
        if let Some(path) = capture {
            let (width, height) = render.drawable_size();
            let pixels = render.read_pixels(width, height)?;
            write_bmp(path, width, height, &pixels)?;
            println!("quadro: {path} ({width}x{height})");
        }

        render.end_frame(&window);

        let error = render.take_gl_error();
        if error != 0 {
            return Err(format!("GL error 0x{error:04x} on frame {frames}").into());
        }

        frames += 1;
        elapsed += 1.0 / 60.0;

        if let Some(limit) = options.frames {
            if frames >= limit {
                report(&mut render, mesh, shader, texture, frames);
                return Ok(());
            }
            continue;
        }

        let spent = started.elapsed().as_millis() as u32;
        if spent < frame_ms {
            platform.delay_ms(frame_ms - spent);
        }
    }
}

fn report(render: &mut RenderContext3D, mesh: u64, shader: u64, texture: Option<u64>, frames: u64) {
    println!("{frames} frames rendered, no GL error");
    render.destroy_mesh(mesh);
    render.destroy_shader(shader);
    if let Some(handle) = texture {
        render.destroy_texture(handle);
    }
    println!(
        "released: {} meshes, {} shaders and {} textures left",
        render.mesh_count(),
        render.shader_count(),
        render.texture_count()
    );
}

/// Writes RGBA8 pixels as a 32-bit BMP, bottom-up.
///
/// The same self-contained writer `cube_demo` uses: BMP needs no image library, and the
/// console builds SDL2_image without PNG.
fn write_bmp(path: &str, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(format!(
            "capture has {} bytes; expected {expected} ({width}x{height} RGBA)",
            rgba.len()
        ));
    }

    let mut pixels = Vec::with_capacity(expected);
    for pixel in rgba.chunks_exact(4) {
        pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }

    let file_size = 14 + 40 + expected as u32;
    let mut out = Vec::with_capacity(file_size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&expected.to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    out.extend_from_slice(&pixels);

    std::fs::write(path, out).map_err(|err| format!("could not write {path}: {err}"))
}
