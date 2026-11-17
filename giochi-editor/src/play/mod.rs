//! Play-in-editor: the scene drawn by the real 3D renderer, in its own window.
//!
//! # Why its own window
//!
//! ADR-037 is explicit: a window carrying `SDL_WINDOW_OPENGL` and a 2D `SDL_Renderer` at the
//! same time makes SDL pick the software path on some drivers, and the editor's panels need
//! the 2D renderer. ADR-064 rejected an FBO for the same reason. So the preview opens a
//! second, OpenGL-only window, and the editor's window stays exactly as it was — which is
//! also what "the editor state is intact" means.
//!
//! On KMSDRM, SDL can only drive one window. [`play`] therefore reports the failure instead
//! of pretending: `Err` carries the driver's own message, and the editor shows it in the
//! status bar. A preview that silently does nothing would be the worst outcome of the three.
//!
//! # What "play" means here
//!
//! The scene is rendered as it is: meshes, colours, the scene's light, and the scene's camera
//! (or one framing the geometry). There is no simulation yet — movement, physics and game
//! logic arrive with the game in phases 41–45, which consume exactly this conversion
//! ([`PlayPlan`]). Calling phase 39 "play" and shipping a walking player would be inventing
//! scope the plan does not have.
//!
//! The user can look around: right-drag orbits, the wheel zooms. `Esc`, the window's ✕ or
//! `--frames` ends the preview and returns to the editor.

pub mod plan;

use std::time::Instant;

use giochi_engine::platform::{Platform, Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::Color3D;
use giochi_engine_3d::render3d::RenderContext3D;
use giochi_engine_3d::scene::Scene;
use giochi_engine_3d::shader::BuiltinShader;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;

use crate::editor::OrbitCamera;

pub use plan::{summary, MeshSpec, PlayCamera, PlayInstance, PlayPlan};

/// How the preview window is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayOptions {
    /// Window width.
    pub width: u32,
    /// Window height.
    pub height: u32,
    /// Window title.
    pub title: String,
    /// Render this many frames and return. `None` runs until the user stops it.
    pub frames: Option<u64>,
}

impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            title: "giochi-editor — play".to_owned(),
            frames: None,
        }
    }
}

/// What the preview did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayStats {
    /// Frames rendered.
    pub frames: u64,
    /// The GL version string the driver gave.
    pub adapter: String,
    /// How the scene was summarised, for the status bar.
    pub summary: String,
}

/// Opens a preview window, draws `scene` until the user stops it, and returns.
///
/// `base` resolves relative `.obj` paths, normally the project root.
///
/// # Errors
///
/// - the video driver refuses a second window (KMSDRM);
/// - GL attributes or the context cannot be created;
/// - a mesh the plan asked for cannot be uploaded.
pub fn play(
    platform: &Platform,
    scene: &Scene,
    base: Option<&std::path::Path>,
    options: PlayOptions,
) -> Result<PlayStats, String> {
    let plan = PlayPlan::from_scene(scene, base);
    let requested = GlesVersion::from_env();
    context::configure_attributes(platform, requested).map_err(|error| error.to_string())?;
    let window = Window::new_opengl(
        platform,
        WindowConfig::new(options.width, options.height),
        &options.title,
    )
    .map_err(|error| error.to_string())?;
    let mut render =
        RenderContext3D::new(context::create(&window).map_err(|error| error.to_string())?);
    let adapter = render.adapter().to_owned();

    // Upload one mesh per distinct spec: ten cubes are one upload and ten draws.
    let shader = render
        .create_builtin_shader(BuiltinShader::Phong)
        .map_err(|error| error.to_string())?;
    let mut meshes = Vec::with_capacity(plan.meshes.len());
    for spec in &plan.meshes {
        let handle = match spec {
            MeshSpec::Obj { path } => render.load_obj_mesh(path),
            _ => match spec.primitive_values() {
                Some(giochi_engine_3d::scene::Primitive::Cube { size }) => {
                    render.create_cube_mesh(size)
                }
                Some(giochi_engine_3d::scene::Primitive::Sphere {
                    radius,
                    rings,
                    segments,
                }) => render.create_sphere_mesh(radius, rings, segments),
                Some(giochi_engine_3d::scene::Primitive::Plane { size, subdivisions }) => {
                    render.create_plane_mesh(size, subdivisions)
                }
                None => continue,
            },
        }
        .map_err(|error| format!("could not upload {spec:?}: {error}"))?;
        meshes.push(handle);
    }
    let handle_for = |spec: &MeshSpec| -> Option<u64> {
        plan.meshes
            .iter()
            .position(|candidate| candidate == spec)
            .and_then(|index| meshes.get(index).copied())
    };

    render.set_light(plan.light);
    render.set_clear_color(Color3D::new(0.07, 0.09, 0.13, 1.0));

    let mut camera = OrbitCamera::looking_at(
        plan.camera.target,
        (plan.camera.position - plan.camera.target).length(),
    );
    camera.target = plan.camera.target;
    camera.fov_y = plan.camera.fov_y;

    let mut frames: u64 = 0;
    let mut last = Instant::now();
    loop {
        let started = Instant::now();
        let dt = (started - last).as_secs_f32();
        last = started;

        for event in platform.poll_events() {
            match event {
                Event::Quit { .. } => return Ok(stats(frames, adapter, &plan)),
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => return Ok(stats(frames, adapter, &plan)),
                Event::MouseMotion {
                    xrel,
                    yrel,
                    mousestate,
                    ..
                } => {
                    if mousestate.right() {
                        camera.orbit(xrel as f32, yrel as f32);
                    }
                }
                Event::MouseWheel { y, .. } => camera.zoom(y as f32),
                _ => {}
            }
        }
        let _ = dt;

        let (width, height) = window.drawable_size();
        let view = camera_camera(&camera, width, height);
        render.begin_frame(&window);
        for instance in &plan.instances {
            let Some(handle) = handle_for(&instance.mesh) else {
                continue;
            };
            render
                .draw_mesh_colored(handle, shader, &instance.transform, &view, instance.color)
                .map_err(|error| error.to_string())?;
        }
        render.end_frame(&window);

        let error = render.take_gl_error();
        if error != 0 {
            return Err(format!("GL error 0x{error:04x} on frame {frames}"));
        }

        frames += 1;
        if options.frames.is_some_and(|limit| frames >= limit) {
            return Ok(stats(frames, adapter, &plan));
        }
    }
}

fn stats(frames: u64, adapter: String, plan: &PlayPlan) -> PlayStats {
    PlayStats {
        frames,
        adapter,
        summary: summary(plan),
    }
}

/// The orbit camera as the 3D renderer's camera, for the window's current size.
fn camera_camera(
    camera: &OrbitCamera,
    width: u32,
    height: u32,
) -> giochi_engine_3d::math3d::Camera3D {
    let play = PlayCamera {
        position: camera.eye(),
        target: camera.target,
        up: camera.up(),
        fov_y: camera.fov_y,
        near: camera.near,
        far: camera.far,
    };
    play.to_camera3d(width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_preview_is_a_whole_window() {
        let options = PlayOptions::default();
        assert_eq!(options.width, 1280);
        assert_eq!(options.height, 720);
        assert!(options.frames.is_none());
    }

    #[test]
    fn the_orbit_camera_becomes_a_3d_camera_with_the_same_framing() {
        let camera = OrbitCamera::default();
        let view = camera_camera(&camera, 1280, 720);
        assert!((view.position.to_glam() - camera.eye()).length() < 1e-4);
        assert!((view.target.to_glam() - camera.target).length() < 1e-4);
        assert!((view.fov_y - camera.fov_y).abs() < 1e-6);
    }
}
