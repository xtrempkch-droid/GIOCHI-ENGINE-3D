//! Pixel-level tests for the directional light.
//!
//! The Phong maths is easy to get wrong in ways that compile and render: an inverted
//! direction lights the scene from below, a `mat3(u_model)` instead of a normal matrix
//! shades a scaled object as a different shape, and a `vec3` uniform filled through
//! `glUniform4fv` raises `GL_INVALID_OPERATION` and leaves the surface black. None of
//! those show up as an error, so these tests read the framebuffer instead.
//!
//! A **sphere** is the probe: its normals cover every direction, so a band of rows
//! above the centre and a band below it are guaranteed to land on surfaces pointing
//! opposite ways. Comparing those two bands answers "which way is the light pointing"
//! without needing to know where the geometry landed on screen.
//!
//! These need a real EGL/GLES driver. `SDL_VIDEODRIVER=offscreen` provides one with no
//! display; without it the tests skip rather than fail.
//!
//! ```text
//! SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d --test lighting
//! ```

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Transform3D, Vec3};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::{DirectionalLight, RenderContext3D};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 120;

/// A headless GL context sized for the probe, or `None` when there is no GL driver.
fn headless_context() -> Option<(Platform, Window)> {
    std::env::set_var("SDL_VIDEODRIVER", "offscreen");

    let platform = Platform::new().ok()?;
    context::configure_attributes(&platform, GlesVersion::Gles2).ok()?;
    let window =
        Window::new_opengl(&platform, WindowConfig::new(WIDTH, HEIGHT), "lighting").ok()?;
    Some((platform, window))
}

/// Average luminance of a patch of the sphere, in `0.0..=255.0`.
///
/// Row 0 of a `glReadPixels` buffer is the **bottom** of the framebuffer, and +Y is up
/// the screen for this camera, so the rows with the largest index hold the top of the
/// sphere. Getting that backwards is how these tests would pass while the light was
/// upside down, so it is spelled out.
///
/// The patch is deliberately narrow and near the vertical axis: with the camera two
/// units from a unit sphere, the silhouette is a circle of about 52 px radius, and the
/// bands picked here are entirely inside it. A band that spilled onto the black
/// background would compare "lit" against "empty" and prove nothing.
fn patch_luma(pixels: &[u8], top: bool) -> f32 {
    let (start_row, end_row) = if top {
        (
            (HEIGHT as f32 * 0.62) as usize,
            (HEIGHT as f32 * 0.78) as usize,
        )
    } else {
        (
            (HEIGHT as f32 * 0.22) as usize,
            (HEIGHT as f32 * 0.38) as usize,
        )
    };
    let centre = WIDTH as usize / 2;
    let half_width = (WIDTH as f32 * 0.15) as usize;

    let mut total = 0.0;
    let mut count = 0.0;
    for y in start_row..end_row {
        for x in (centre - half_width)..(centre + half_width) {
            let at = (y * WIDTH as usize + x) * 4;
            let r = pixels[at] as f32;
            let g = pixels[at + 1] as f32;
            let b = pixels[at + 2] as f32;
            // Rec. 601 luma: the eye is not equally sensitive to the channels, and the
            // default light is white so the weighting is the honest one.
            total += 0.299 * r + 0.587 * g + 0.114 * b;
            count += 1.0;
        }
    }

    total / count
}

/// The brightest single channel anywhere in the frame.
fn peak_luma(pixels: &[u8]) -> f32 {
    pixels
        .chunks_exact(4)
        .flat_map(|px| [px[0], px[1], px[2]])
        .map(f32::from)
        .fold(0.0, f32::max)
}

struct Scene {
    render: RenderContext3D,
    sphere: u64,
    shader: u64,
    camera: Camera3D,
    window: Window,
    _platform: Platform,
}

impl Scene {
    fn new() -> Option<Self> {
        let (platform, window) = headless_context()?;
        let mut render = RenderContext3D::new(context::create(&window).ok()?);

        let sphere = render.create_sphere_mesh(1.0, 24, 24).ok()?;
        let shader = render.create_builtin_shader(BuiltinShader::Phong).ok()?;
        // Two units from a unit sphere: close enough that the silhouette covers most of
        // the frame, so a patch near the axis is certain to be on the surface.
        let camera = Camera3D::from_parts(Vec3::new(0.0, 0.0, 2.0), Vec3::ZERO, Vec3::UP)
            .with_clip_planes(0.1, 100.0);

        Some(Self {
            render,
            sphere,
            shader,
            camera,
            window,
            _platform: platform,
        })
    }

    /// Draws the sphere under `light` and returns the whole framebuffer.
    fn capture(&mut self, light: DirectionalLight, colour: Color3D) -> Vec<u8> {
        self.render.set_light(light);
        self.render.set_clear_color(Color3D::BLACK);

        self.render.begin_frame(&self.window);
        // Pinned explicitly: on some drivers the offscreen window does not come back the
        // size that was asked for, and the camera aspect is derived from this.
        self.render.resize(WIDTH, HEIGHT);

        self.render
            .draw_mesh_colored(
                self.sphere,
                self.shader,
                &Transform3D::IDENTITY,
                &self.camera,
                colour,
            )
            .expect("drawing a sphere must succeed");

        let error = self.render.take_gl_error();
        assert_eq!(
            error, 0,
            "GL error 0x{error:04x} while drawing the lit sphere"
        );

        self.render
            .read_pixels(WIDTH, HEIGHT)
            .expect("read back the framebuffer")
    }
}

macro_rules! scene_or_skip {
    () => {
        match Scene::new() {
            Some(scene) => scene,
            None => {
                eprintln!("skipped: no OpenGL ES driver available");
                return;
            }
        }
    };
}

#[test]
fn a_light_pointing_down_lights_the_top_of_the_sphere() {
    let mut scene = scene_or_skip!();

    // `direction` is the way the light travels, so (0, -1, 0) is a sun overhead.
    let pixels = scene.capture(
        DirectionalLight::sun()
            .with_direction(Vec3::new(0.0, -1.0, 0.0))
            .with_ambient(0.05)
            .with_specular(0.0, 1.0),
        Color3D::WHITE,
    );

    let top = patch_luma(&pixels, true);
    let bottom = patch_luma(&pixels, false);

    assert!(
        top > bottom + 20.0,
        "a light travelling downwards must brighten the upper half of the sphere: \
         top={top:.1}, bottom={bottom:.1}"
    );
}

#[test]
fn flipping_the_light_flips_the_shading() {
    let mut scene = scene_or_skip!();

    let down = scene.capture(
        DirectionalLight::sun()
            .with_direction(Vec3::new(0.0, -1.0, 0.0))
            .with_ambient(0.05)
            .with_specular(0.0, 1.0),
        Color3D::WHITE,
    );
    let up = scene.capture(
        DirectionalLight::sun()
            .with_direction(Vec3::new(0.0, 1.0, 0.0))
            .with_ambient(0.05)
            .with_specular(0.0, 1.0),
        Color3D::WHITE,
    );

    // This is the assertion that catches a missing negation of `u_light_direction` in
    // the fragment shader: with the sign wrong, both captures come out the same way up
    // and the test above still passes.
    assert!(
        patch_luma(&down, true) > patch_luma(&down, false) + 20.0,
        "the down light must favour the upper half"
    );
    assert!(
        patch_luma(&up, false) > patch_luma(&up, true) + 20.0,
        "the up light must favour the lower half: up light top={:.1}, bottom={:.1}",
        patch_luma(&up, true),
        patch_luma(&up, false)
    );
}

#[test]
fn ambient_alone_still_shows_the_shape() {
    let mut scene = scene_or_skip!();

    // Ambient is the only term that does not depend on the light direction, so with the
    // diffuse and specular contributions removed the whole sphere must be the surface
    // colour times the ambient strength -- visible, and flat.
    let dim = scene.capture(
        DirectionalLight::sun()
            .with_direction(Vec3::new(0.0, -1.0, 0.0))
            .with_ambient(0.1)
            .with_specular(0.0, 1.0),
        Color3D::WHITE,
    );
    let bright = scene.capture(
        DirectionalLight::sun()
            .with_direction(Vec3::new(0.0, -1.0, 0.0))
            .with_ambient(0.6)
            .with_specular(0.0, 1.0),
        Color3D::WHITE,
    );

    let shadowed_dim = patch_luma(&dim, false);
    let shadowed_bright = patch_luma(&bright, false);

    assert!(
        shadowed_bright > shadowed_dim + 20.0,
        "raising the ambient must lift the unlit side: {shadowed_dim:.1} -> {shadowed_bright:.1}"
    );
    // The epsilon matters: `clamped()` keeps the colour in range, so 0.6 ambient is a
    // mid grey, not a white.
    assert!(
        shadowed_bright < 255.0,
        "0.6 ambient must not saturate the surface: {shadowed_bright:.1}"
    );
}

#[test]
fn the_specular_term_adds_a_highlight() {
    let mut scene = scene_or_skip!();

    // A light straight down and a camera straight on puts the highlight near the top of
    // the sphere. Comparing the peak instead of a band keeps this independent of where
    // exactly it lands.
    let matte = scene.capture(
        DirectionalLight::sun()
            .with_direction(Vec3::new(0.0, -1.0, 0.0))
            .with_ambient(0.05)
            .with_specular(0.0, 1.0),
        Color3D::new(0.4, 0.4, 0.4, 1.0),
    );
    let glossy = scene.capture(
        DirectionalLight::sun()
            .with_direction(Vec3::new(0.0, -1.0, 0.0))
            .with_ambient(0.05)
            .with_specular(1.0, 16.0),
        Color3D::new(0.4, 0.4, 0.4, 1.0),
    );

    let matte_peak = peak_luma(&matte);
    let glossy_peak = peak_luma(&glossy);

    assert!(
        glossy_peak > matte_peak + 10.0,
        "the specular term must be visible: matte peak={matte_peak:.1}, glossy peak={glossy_peak:.1}"
    );
}

#[test]
fn a_coloured_light_tints_the_surface() {
    let mut scene = scene_or_skip!();

    // A red light on a white surface must reproduce the white light's red channel
    // exactly and remove the other two. Comparing against the white capture is what
    // makes this about the *colour* rather than about the local diffuse term, which
    // varies across a sphere.
    let white = scene.capture(
        DirectionalLight::sun()
            .with_direction(Vec3::new(0.0, -1.0, 0.0))
            .with_ambient(0.0)
            .with_specular(0.0, 1.0),
        Color3D::WHITE,
    );
    let red = scene.capture(
        DirectionalLight::sun()
            .with_direction(Vec3::new(0.0, -1.0, 0.0))
            .with_ambient(0.0)
            .with_specular(0.0, 1.0)
            .with_color(Color3D::new(1.0, 0.0, 0.0, 1.0)),
        Color3D::WHITE,
    );

    // A point on the lit upper hemisphere, on the vertical axis.
    let at = (HEIGHT as usize * 70 / 100 * WIDTH as usize + WIDTH as usize / 2) * 4;

    assert!(
        white[at + 1] > 20 && white[at + 2] > 20,
        "the white light must be neutral to compare against: got r={} g={} b={}",
        white[at],
        white[at + 1],
        white[at + 2]
    );
    assert!(
        (red[at] as i32 - white[at] as i32).abs() <= 2,
        "the red channel must be unchanged by a red light: white={} red={}",
        white[at],
        red[at]
    );
    // A `u_light_color` uploaded as a `vec4` into a `vec3` uniform is rejected by the
    // driver, the uniform stays at zero and the whole sphere comes out black, so this
    // is the assertion that catches that.
    assert!(
        red[at + 1] < 8 && red[at + 2] < 8,
        "a pure red light must leave the green and blue channels dark: \
         got r={} g={} b={}",
        red[at],
        red[at + 1],
        red[at + 2]
    );
}

#[test]
fn a_cube_lit_from_above_shows_a_bright_top_face() {
    let Some((_platform, window)) = headless_context() else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("context"));
    let cube = render.create_cube_mesh(1.0).expect("cube");
    let shader = render
        .create_builtin_shader(BuiltinShader::Phong)
        .expect("phong");
    // High enough to see the top face and the front face at the same time.
    let camera = Camera3D::from_parts(Vec3::new(0.0, 2.0, 3.0), Vec3::ZERO, Vec3::UP)
        .with_clip_planes(0.1, 100.0);

    let capture = |render: &mut RenderContext3D, light: DirectionalLight| {
        render.set_light(light);
        render.set_clear_color(Color3D::BLACK);
        render.begin_frame(&window);
        render.resize(WIDTH, HEIGHT);
        render
            .draw_mesh_colored(
                cube,
                shader,
                &Transform3D::IDENTITY,
                &camera,
                Color3D::WHITE,
            )
            .expect("draw");
        let error = render.take_gl_error();
        assert_eq!(
            error, 0,
            "GL error 0x{error:04x} while drawing the lit cube"
        );
        render.read_pixels(WIDTH, HEIGHT).expect("read back")
    };

    // The top face's normal is exactly +Y and the light travels exactly down, so
    // `dot(normal, to_light)` is exactly 1: with ambient at zero the top face must
    // saturate. This is the phase-13 acceptance criterion, "a face on top is brighter
    // than a side face", stated as a value rather than as a comparison between two
    // hand-picked pixels.
    let from_above = capture(
        &mut render,
        DirectionalLight::sun()
            .with_direction(Vec3::new(0.0, -1.0, 0.0))
            .with_ambient(0.0)
            .with_specular(0.0, 1.0),
    );
    assert!(
        peak_luma(&from_above) > 240.0,
        "the top face must saturate under a light pointing straight down: peak={:.1}",
        peak_luma(&from_above)
    );

    // The same cube with the light travelling *upwards*: no face the camera can see
    // points down, so with no ambient the frame must be black. If the fragment shader
    // forgot to negate the direction, this capture would look exactly like the one
    // above and the test would fail here.
    let from_below = capture(
        &mut render,
        DirectionalLight::sun()
            .with_direction(Vec3::new(0.0, 1.0, 0.0))
            .with_ambient(0.0)
            .with_specular(0.0, 1.0),
    );
    assert!(
        peak_luma(&from_below) < 8.0,
        "a light travelling upwards must leave the visible faces dark: peak={:.1}",
        peak_luma(&from_below)
    );
}

/// Mean luma of a small square patch centred on a pixel.
fn patch_at(pixels: &[u8], x: usize, y: usize, half: usize) -> f32 {
    let mut total = 0.0;
    let mut count = 0.0;
    for py in y.saturating_sub(half)..=(y + half).min(HEIGHT as usize - 1) {
        for px in x.saturating_sub(half)..=(x + half).min(WIDTH as usize - 1) {
            let at = (py * WIDTH as usize + px) * 4;
            total += 0.299 * pixels[at] as f32
                + 0.587 * pixels[at + 1] as f32
                + 0.114 * pixels[at + 2] as f32;
            count += 1.0;
        }
    }
    if count == 0.0 {
        0.0
    } else {
        total / count
    }
}

/// Where a world point lands, in `glReadPixels` pixel coordinates (row 0 is the bottom).
fn to_pixels(camera: &Camera3D, point: Vec3) -> (usize, usize) {
    let ndc = camera
        .with_drawable_size(WIDTH, HEIGHT)
        .view_projection()
        .expect("valid camera")
        .project_point(point)
        .unwrap_or_else(|| panic!("{point:?} is behind the camera"));

    let x = ((ndc.x * 0.5 + 0.5) * WIDTH as f32) as usize;
    // ndc.y of -1 is the bottom of the frame, which is row 0 of the readback.
    let y = ((ndc.y * 0.5 + 0.5) * HEIGHT as f32) as usize;
    (x.min(WIDTH as usize - 1), y.min(HEIGHT as usize - 1))
}

#[test]
fn a_crates_top_face_reads_brighter_than_its_side_the_phase_13_acceptance_check() {
    let Some((_platform, window)) = headless_context() else {
        eprintln!("skipped: no OpenGL ES driver available");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("context"));
    let shader = render
        .create_builtin_shader(BuiltinShader::Phong)
        .expect("phong");

    // `mini_fps`'s crate, its eye height, and its light. The camera stands closer than the
    // walker happens to be so the crate fills enough of the frame to sample: at the probe
    // resolution a crate 6 units away is 20-odd pixels tall and the two faces are within a
    // patch of each other.
    let crate_size = 0.6_f32;
    let crate_mesh = render.create_cube_mesh(crate_size).expect("cube");
    let camera = Camera3D::from_parts(Vec3::new(0.0, 1.7, 1.6), Vec3::new(0.0, 0.3, 0.0), Vec3::UP)
        .with_clip_planes(0.1, 100.0);
    let light = DirectionalLight::sun()
        .with_direction(Vec3::new(0.4, -1.0, 0.3))
        .with_specular(0.3, 24.0);

    render.set_light(light);
    render.set_clear_color(Color3D::BLACK);
    render.begin_frame(&window);
    render.resize(WIDTH, HEIGHT);
    render
        .draw_mesh_colored(
            crate_mesh,
            shader,
            &Transform3D::from_translation(Vec3::new(0.0, crate_size * 0.5, 0.0)),
            &camera,
            Color3D::WHITE,
        )
        .expect("draw");
    let error = render.take_gl_error();
    assert_eq!(error, 0, "GL error 0x{error:04x} drawing the crate");
    let pixels = render.read_pixels(WIDTH, HEIGHT).expect("read back");

    let half = crate_size * 0.5;
    // Centres of the two faces, chosen from the geometry rather than by eye: the top face
    // is the plane y = crate_size, the front face the plane z = +half.
    let (top_x, top_y) = to_pixels(&camera, Vec3::new(0.0, crate_size, 0.0));
    let (side_x, side_y) = to_pixels(&camera, Vec3::new(0.0, crate_size * 0.5, half));

    // Patches that overlap measure the seam between two faces and report the average of
    // both, which is how this test first passed locally by 1 unit of luma and then failed
    // on the runner. The framing is checked before it is trusted.
    const PATCH: usize = 3;
    let separation =
        ((top_x as f64 - side_x as f64).powi(2) + (top_y as f64 - side_y as f64).powi(2)).sqrt();
    assert!(
        separation > (PATCH * 2 + 2) as f64,
        "the two sample patches overlap: top ({top_x},{top_y}) and front ({side_x},{side_y}) \
         are {separation:.1} px apart, which cannot separate two {PATCH}-px patches. \
         Move the camera closer or the crate further from the eye."
    );

    let top = patch_at(&pixels, top_x, top_y, PATCH);
    let side = patch_at(&pixels, side_x, side_y, PATCH);

    // "Face superior mais clara que lateral", as a measurement. The margin is wide because
    // the difference is: the sun is 0.89 of the way to straight up, so the top face gets
    // ambient + 0.7 * 0.89 while the front face's normal points away from the light and
    // gets ambient alone -- roughly a factor of three.
    assert!(
        top > side * 1.5,
        "the top face must read brighter than the front face: \
         top ({top_x},{top_y})={top:.1}, front ({side_x},{side_y})={side:.1}"
    );
    // And both must be on the crate, not on the black background behind it.
    assert!(top > 60.0, "the top patch is on the background: {top:.1}");
    assert!(
        side > 20.0,
        "the front patch is on the background: {side:.1}"
    );
}

#[test]
fn the_light_is_sanitised_before_it_reaches_the_shader() {
    let mut scene = scene_or_skip!();

    // A degenerate direction would normalise to NaN. Most drivers render NaN as black,
    // so the honest behaviour is to keep the frame lit rather than to hand the shader a
    // value it cannot use.
    scene
        .render
        .set_light(DirectionalLight::sun().with_direction(Vec3::ZERO));

    let light = scene.render.light();
    assert!(light.direction.is_finite());
    assert!((light.direction.length() - 1.0).abs() < 1e-5);
    assert!(light.direction.y < 0.0);
}
