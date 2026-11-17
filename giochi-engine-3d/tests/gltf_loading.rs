//! Loading a glTF file from disk, the way a game does it.
//!
//! The unit tests next to the loader cover the formats (GLB, `data:` URIs, transforms). This
//! one covers what only a real file can: a `.gltf` next to its own `.bin`, resolved against
//! the file's directory rather than the process's working directory.
//!
//! No GL context is involved: `load_gltf` produces vertices, and a test that needed a GPU to
//! check a parser would skip on exactly the machine where the parser matters.

use std::path::PathBuf;

use giochi_engine_3d::mesh::load_gltf;

/// Writes a two-triangle quad as `.gltf` + `.bin` and returns the folder.
fn write_quad(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "giochi-gltf-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");

    // Four corners of a unit quad in the XY plane, counter-clockwise seen from +Z.
    let positions: [[f32; 3]; 4] = [
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
    ];
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    let mut bin = Vec::new();
    for position in positions {
        for value in position {
            bin.extend_from_slice(&value.to_le_bytes());
        }
    }
    let index_offset = bin.len();
    for index in indices {
        bin.extend_from_slice(&index.to_le_bytes());
    }
    // Pad to four bytes, which the spec asks for and some validators insist on.
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    std::fs::write(dir.join("quad.bin"), &bin).expect("bin");

    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0", "generator": "giochi-engine-3d test" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0] }}],
  "nodes": [{{ "mesh": 0, "translation": [0.0, 1.0, 0.0] }}],
  "meshes": [{{
    "name": "quad",
    "primitives": [{{
      "attributes": {{ "POSITION": 0 }},
      "indices": 1,
      "material": 0
    }}]
  }}],
  "materials": [{{
    "name": "paint",
    "pbrMetallicRoughness": {{
      "baseColorFactor": [0.2, 0.4, 0.9, 1.0],
      "metallicFactor": 0.0,
      "roughnessFactor": 0.35
    }},
    "emissiveFactor": [0.1, 0.0, 0.0]
  }}],
  "accessors": [
    {{
      "bufferView": 0,
      "componentType": 5126,
      "count": 4,
      "type": "VEC3",
      "min": [-1.0, -1.0, 0.0],
      "max": [1.0, 1.0, 0.0]
    }},
    {{
      "bufferView": 1,
      "componentType": 5123,
      "count": 6,
      "type": "SCALAR"
    }}
  ],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": {index_offset}, "byteLength": 12 }}
  ],
  "buffers": [{{ "byteLength": {length}, "uri": "quad.bin" }}]
}}"#,
        index_offset = index_offset,
        length = bin.len()
    );
    std::fs::write(dir.join("quad.gltf"), json).expect("gltf");
    dir
}

#[test]
fn a_gltf_next_to_its_bin_loads_from_any_working_directory() {
    let dir = write_quad("quad");
    let path = dir.join("quad.gltf");

    // The loader must resolve `quad.bin` against the model's own folder. Run from a
    // directory that has no `quad.bin` at all: a loader that used the process's working
    // directory would fail here.
    let elsewhere = std::env::temp_dir();
    let previous = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&elsewhere).expect("chdir");
    let model = load_gltf(&path);
    std::env::set_current_dir(previous).expect("chdir back");

    let model = model.expect("the model loads");
    assert_eq!(model.primitives.len(), 1);
    let primitive = &model.primitives[0];
    assert_eq!(primitive.vertices.len(), 4);
    assert_eq!(primitive.indices, vec![0, 1, 2, 0, 2, 3]);
    assert_eq!(primitive.triangle_count(), 2);

    // The node moved the quad up by one: the accessor says -1..1, the vertices say 0..2.
    let bounds = model.bounds.expect("bounds");
    assert!((bounds.0[1]).abs() < 1e-5, "{bounds:?}");
    assert!((bounds.1[1] - 2.0).abs() < 1e-5, "{bounds:?}");
    assert!((bounds.0[0] + 1.0).abs() < 1e-5, "{bounds:?}");

    // The material survived the round trip through the file.
    let material = primitive.material.as_ref().expect("material");
    assert_eq!(material.base_color, [0.2, 0.4, 0.9, 1.0]);
    assert_eq!(material.emissive, [0.1, 0.0, 0.0]);
    assert!((material.roughness - 0.35).abs() < 1e-6);

    // Normals were computed because the file has none, and a flat quad in the XY plane
    // faces +Z.
    for vertex in &primitive.vertices {
        assert!((vertex.normal[2] - 1.0).abs() < 1e-5, "{:?}", vertex.normal);
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn loading_a_file_that_is_not_there_names_the_path() {
    let error = load_gltf(std::path::Path::new(
        "/tmp/giochi-does-not-exist/scene.gltf",
    ))
    .expect_err("must fail");
    let text = format!("{error}");
    assert!(text.contains("scene.gltf"), "{text}");
}

#[test]
fn a_file_that_is_not_a_model_is_refused_with_a_reason() {
    let dir = write_quad("garbage");
    let path = dir.join("garbage.gltf");
    std::fs::write(&path, b"{\"asset\": {\"version\": \"1.0\"}}").expect("written");
    let error = load_gltf(&path).expect_err("must fail");
    assert!(!format!("{error}").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Renderer -----------------------------------------------------------------
//
// The rest of this file is a parser test. This part proves the piece a game actually calls:
// `load_gltf_meshes` uploads each primitive and the result draws. It needs a GL driver, so it
// skips itself (loudly) where there is none — the same rule the engine's own render tests
// follow.

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Transform3D, Vec3};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;

#[test]
fn an_uploaded_gltf_draws_into_the_framebuffer() {
    if std::env::var_os("SDL_VIDEODRIVER").is_none() {
        std::env::set_var("SDL_VIDEODRIVER", "offscreen");
    }
    let Ok(platform) = Platform::new() else {
        eprintln!("skipped: no SDL platform");
        return;
    };
    if context::configure_attributes(&platform, GlesVersion::Gles2).is_err() {
        eprintln!("skipped: no GL attributes");
        return;
    }
    let Ok(window) = Window::new_opengl(&platform, WindowConfig::new(128, 128), "gltf-draw") else {
        eprintln!("skipped: no GL window");
        return;
    };
    let Ok(gl) = context::create(&window) else {
        eprintln!("skipped: no GL context");
        return;
    };
    let mut render = RenderContext3D::new(gl);

    let dir = write_quad("draw");
    let loaded = render
        .load_gltf_meshes(&dir.join("quad.gltf"))
        .expect("the model uploads");
    assert_eq!(loaded.mesh_count(), 1);
    assert!(loaded.bounds().is_some());

    // The quad's material is blue with a red emissive; either channel proves the draw
    // reached the framebuffer, and the emissive specifically proves the new uniform is
    // uploaded by the real path.
    let shader = render
        .create_builtin_shader(BuiltinShader::Phong)
        .expect("shader");
    render.set_clear_color(Color3D::new(0.0, 0.0, 0.0, 1.0));
    let camera = Camera3D::from_parts(
        Vec3::new(0.0, 1.0, 4.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    )
    .with_drawable_size(128, 128);

    render.begin_frame(&window);
    let (handle, material) = &loaded.primitives[0];
    let material = material.as_ref().expect("the file names a material");
    render
        .draw_mesh_material(
            *handle,
            shader,
            &Transform3D::IDENTITY,
            &camera,
            &material.to_material(),
        )
        .expect("drawn");
    let pixels = render.read_pixels(128, 128).expect("pixels");
    render.end_frame(&window);

    let lit = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] > 8 || pixel[1] > 8 || pixel[2] > 8)
        .count();
    assert!(lit > 200, "{lit} lit pixels: the model did not draw");

    // A frame that draws *and* leaves an error behind is a frame that will fail on the next
    // driver. This specific check caught `u_emissive` being uploaded as a `vec4` to a `vec3`
    // uniform: legal on paper, `GL_INVALID_OPERATION` on radeonsi.
    let error = render.take_gl_error();
    assert_eq!(error, 0, "the draw left GL error 0x{error:04x}");
    let _ = std::fs::remove_dir_all(&dir);
}
