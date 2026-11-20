//! Hot reload never takes the game down with it (DOD criteria 3 and 4).
//!
//! **Ativado na Fase S4.**
//!
//! # The contract, in one sentence
//!
//! A bad file on disk must leave the previous good state **active**, report the failure to
//! the caller, log it, and change nothing else.
//!
//! # Why the current API can be tested before S4
//!
//! [`ShaderReloader::reload_shader`] returns `bool` today. S4 decides whether that becomes
//! `Result<(), ReloadError>`; either way the observable property is the same, so the tests
//! below are written against it and do not assume the signature. That is deliberate: a DOD
//! criterion that names a return type would force a refactor to satisfy a test.
//!
//! The scene half uses [`Scene::from_json`], which already returns `Result` — a scene that
//! fails to parse cannot replace the one the editor is editing, and that is asserted here
//! today so S4 only has to keep it true for the loader path.

mod common;

use std::io::Write;
use std::sync::MutexGuard;

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::scene::Scene;
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;

const SIZE: u32 = 64;

/// A file in the temp directory that is removed when the test returns.
struct TempFile(std::path::PathBuf);

impl TempFile {
    fn new(name: &str, contents: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("giochi-stability-{}-{name}", std::process::id()));
        let mut file = std::fs::File::create(&path).expect("create temp file");
        file.write_all(contents.as_bytes())
            .expect("write temp file");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

type Gl = (MutexGuard<'static, ()>, Platform, Window, RenderContext3D);

fn headless() -> Option<Gl> {
    let guard = common::platform_guard();
    common::set_headless_driver();
    let platform = Platform::new().ok()?;
    context::configure_attributes(&platform, GlesVersion::Gles2).ok()?;
    let window = Window::new_opengl(&platform, WindowConfig::new(SIZE, SIZE), "reload").ok()?;
    let render = RenderContext3D::new(context::create(&window).ok()?);
    Some((guard, platform, window, render))
}

#[test]
#[ignore = "Ativado na Fase S4"]
fn a_shader_with_a_syntax_error_keeps_the_previous_one_active() {
    // This is the core of R4. The reloader is fed a file, not a string, so the bad
    // version is written to disk exactly as an editor would.
    let good = TempFile::new(
        "good.vert",
        "#version 100\nattribute vec3 a_position;\nvoid main() { gl_Position = vec4(a_position, 1.0); }\n",
    );
    let bad = TempFile::new(
        "bad.frag",
        "#version 100\nprecision mediump float;\nvoid main() { this is not GLSL }\n",
    );

    let Some((_guard, _platform, window, mut render)) = headless() else {
        eprintln!("skipped: no GL driver");
        return;
    };

    let before = render.shader_count();
    let shader = render
        .create_builtin_shader(BuiltinShader::Flat)
        .expect("a builtin shader");

    // A broken fragment source must be refused, and the shader that is already compiled
    // must still be the one the renderer holds.
    let replaced = render
        .reload_shader_sources(shader, good.path(), bad.path())
        .is_ok();
    assert!(!replaced, "the renderer accepted unparsable GLSL");
    assert_eq!(render.shader_count(), before + 1, "the shader was lost");

    render.clear(&window, giochi_engine_3d::math3d::Color3D::BLACK);
    assert_eq!(
        render.take_gl_error(),
        0,
        "a GL error escaped the failed reload"
    );
}

#[test]
#[ignore = "Ativado na Fase S4"]
fn a_scene_with_invalid_json_does_not_replace_the_current_one() {
    // `Scene::from_json` already refuses; S4 must keep the loader path doing the same.
    let original = Scene::default_scene();
    let invalid = Scene::from_json("{ this is not json");
    assert!(invalid.is_err(), "invalid JSON parsed as a scene");
    // The value the editor is holding is untouched: `from_json` takes `&str` and returns
    // a new value, which is what makes that true by construction.
    assert_eq!(original.nodes.len(), Scene::default_scene().nodes.len());
}

#[test]
#[ignore = "Ativado na Fase S4"]
fn a_scene_from_a_future_version_is_refused_rather_than_misread() {
    let text = r#"{"format":"giochi-scene","version":9999,"name":"future","next_id":1,"nodes":[]}"#;
    assert!(
        Scene::from_json(text).is_err(),
        "a future scene version was accepted"
    );
}

#[test]
#[ignore = "Ativado na Fase S4"]
fn a_corrupted_texture_keeps_the_previous_texture() {
    todo!("S4: reload path for textures — warn!, keep previous, never panic")
}

#[test]
#[ignore = "Ativado na Fase S4"]
fn ten_consecutive_failed_reloads_leave_the_renderer_usable() {
    todo!("S4: the editor-Play-after-10-reloads acceptance, as a headless loop")
}
