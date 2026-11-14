//! Headless integration test for hot-reloading a texture from disk.
//!
//! One test per binary, for the same reason as `render_smoke`: SDL allows a single
//! `EventPump` per process and cargo runs one binary's tests concurrently.
//!
//! The image is a **BMP**, written by this file, because BMP needs no decoder flags in
//! SDL_image and no PNG encoder in the test's dependency tree. That matters on the
//! console image, where the PNG decoder is optional (see `TextureRegistry`'s
//! `ensure_image_subsystem`).

use std::path::{Path, PathBuf};

use giochi_engine::{codes, Color, Platform, Rect, RenderContext, WindowConfig};

/// Writes a `width`x`height` 24-bit BMP filled with `color`.
fn write_bmp(path: &Path, width: u32, height: u32, color: (u8, u8, u8)) {
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

    // BMP rows are bottom-up and pixels are BGR.
    let padding = row_bytes - width * 3;
    for _ in 0..height {
        for _ in 0..width {
            bytes.extend_from_slice(&[color.2, color.1, color.0]);
        }
        bytes.extend(std::iter::repeat_n(0u8, padding as usize));
    }

    std::fs::write(path, bytes).expect("the test must be able to write its fixture");
}

fn asset(name: &str) -> String {
    format!("{}/../assets/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn a_saved_texture_reloads_in_place_without_a_new_handle() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");

    let dir: PathBuf = std::env::temp_dir().join(format!(
        "giochi-hot-texture-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let image = dir.join("swatch.bmp");

    let red = (220u8, 30u8, 40u8);
    let blue = (20u8, 60u8, 220u8);
    write_bmp(&image, 16, 16, red);

    let platform = Platform::new().expect("dummy driver must initialise");
    let window = platform
        .create_window(WindowConfig::new(32, 32), "giochi-hot-reload-test")
        .expect("window must be created");
    let mut render = RenderContext::new(window).expect("renderer must be created");

    let path = image.to_string_lossy().to_string();
    let handle = render.create_texture(&path).expect("BMP must load");
    assert_eq!(render.texture_size(handle), Some((16, 16)));
    assert_eq!(
        render.texture_source(handle).as_deref(),
        Some(path.as_str()),
        "the registry must remember where the texture came from"
    );

    // --- the pixels on screen are the ones on disk -------------------------
    render.clear(Color::rgb(0, 0, 0));
    render
        .draw_texture(handle, Rect::new(0.0, 0.0, 32.0, 32.0), Rect::ZERO)
        .expect("draw must succeed");
    let before = render
        .read_pixels(Rect::new(16.0, 16.0, 1.0, 1.0))
        .expect("read_pixels must work");
    assert_eq!(&before[..3], &[red.0, red.1, red.2]);

    // --- save a different image, reload, see the new one -------------------
    write_bmp(&image, 16, 16, blue);
    render.reload_texture(handle).expect("reload must succeed");

    assert_eq!(
        render.texture_size(handle),
        Some((16, 16)),
        "the handle must survive the reload"
    );
    assert_eq!(
        render.texture_count(),
        1,
        "a reload must not leak a texture"
    );

    render.clear(Color::rgb(0, 0, 0));
    render
        .draw_texture(handle, Rect::new(0.0, 0.0, 32.0, 32.0), Rect::ZERO)
        .expect("draw must succeed");
    let after = render
        .read_pixels(Rect::new(16.0, 16.0, 1.0, 1.0))
        .expect("read_pixels must work");
    assert_eq!(
        &after[..3],
        &[blue.0, blue.1, blue.2],
        "the reloaded texture must be the one now on disk"
    );

    // --- reloading by path finds the same handle ---------------------------
    write_bmp(&image, 16, 16, red);
    assert_eq!(
        render.reload_texture_path(&image),
        1,
        "the path must map back to the handle"
    );

    // --- a file that is gone keeps the previous image ----------------------
    std::fs::remove_file(&image).expect("remove fixture");
    let error = render
        .reload_texture(handle)
        .expect_err("a missing file must not be a silent success");
    assert_eq!(error.code(), codes::ASSET);
    assert_eq!(
        render.texture_size(handle),
        Some((16, 16)),
        "a failed reload must leave the old texture alive"
    );

    // --- a generated texture has no source to reload from ------------------
    let generated = render
        .create_texture_from_rgba(2, 2, &[255u8; 16])
        .expect("generated texture");
    let error = render
        .reload_texture(generated)
        .expect_err("there is no file to reload from");
    assert_eq!(error.code(), codes::INVALID_ARGUMENT);

    // --- reloading a handle that never existed is reported -----------------
    let error = render.reload_texture(0).expect_err("handle 0 is invalid");
    assert_eq!(error.code(), codes::INVALID_ARGUMENT);

    // --- destroying the texture forgets the path ---------------------------
    assert!(render.destroy_texture(handle));
    assert_eq!(render.texture_source(handle), None);
    assert_eq!(
        render.texture_count(),
        1,
        "only the generated texture is left"
    );

    let _ = std::fs::remove_dir_all(&dir);

    // ---- the resource cache ------------------------------------------------
    //
    // `checker.png` ships with the repository, so nothing has to be written here.
    let cached_path = asset("checker.png");
    let cached = render
        .load_texture(&cached_path)
        .expect("checker.png must load");
    assert!(render
        .resources()
        .is_texture_cached(render.textures(), &cached_path));
    assert_eq!(
        render.resources().path_for_handle(cached).as_deref(),
        Some(cached_path.as_str())
    );

    // Reloading through the context keeps the handle, so the cache stays valid.
    assert_eq!(
        render
            .reload_cached_texture(&cached_path)
            .expect("a cached path must reload"),
        cached
    );
    assert_eq!(
        render.texture_count(),
        2,
        "the generated one and the checker"
    );

    // An uncached path is refused rather than invented.
    let error = render
        .reload_cached_texture("assets/never-loaded.png")
        .expect_err("an uncached path has nothing to reload");
    assert_eq!(error.code(), codes::INVALID_ARGUMENT);

    render.textures().clear();
}
