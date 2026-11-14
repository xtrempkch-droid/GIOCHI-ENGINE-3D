//! The 2D rendering context.

use std::path::Path;
use std::rc::Rc;

use sdl2::render::{TextureCreator, WindowCanvas};
use sdl2::video::Window as SdlWindow;
use sdl2::video::WindowContext;

use crate::assets::ResourceManager;
use crate::error::GiochiError;
use crate::math::{Color, Rect, Vec2};
use crate::perf::FrameCounters;
use crate::platform::sdl::SdlContext;
use crate::platform::window::Window;
use crate::render::atlas::TextureAtlas;
use crate::render::batch::{RunKind, SpriteBatch, SpriteBatchConfig};
use crate::render::camera::Camera;
use crate::render::sprite::Sprite;
use crate::render::texture::{TextureHandle, TextureRegistry};

/// The engine's 2D render context.
///
/// Thin wrapper over the SDL2 canvas: it owns the drawing target and offers the
/// only drawing primitives the engine has. `draw_rect` and `draw_texture` work in
/// **screen** space; `draw_sprite` treats the sprite rectangle as **world** space
/// and applies the current [`Camera`].
///
/// Textured draws are **batched**: consecutive sprites that share a texture become one
/// backend call, flushed on [`RenderContext::present`], on any call that observes the
/// target ([`RenderContext::read_pixels`]), and before an unbatched primitive so
/// submission order is preserved. [`RenderContext::draw_texture`] keeps its signature.
/// See ADR-043.
///
/// Creating the context consumes the [`Window`], because SDL binds the renderer to
/// its window; the context takes over the window-level operations it can provide
/// (`size`, `drawable_size`, `set_title`).
///
/// # C ABI
///
/// Not exposed. C consumers call `giochi_render_*`, which operate on the engine's
/// single context (ADR-012).
#[repr(C)]
pub struct RenderContext {
    // Dropped before `_context`: the canvas must be destroyed while SDL is still
    // initialised.
    canvas: WindowCanvas,
    // `Rc` so the C ABI can hold the registry independently of the canvas: every
    // `TextureRegistry` method takes `&self`, so a shared copy lets `giochi_texture_*`
    // work from inside a scene callback, where the engine is already borrowed.
    textures: Rc<TextureRegistry>,
    resources: ResourceManager,
    camera: Camera,
    counters: FrameCounters,
    batch: SpriteBatch,
    /// Window size cached for culling; refreshed by [`RenderContext::clear`].
    window_size: (u32, u32),
    _context: Rc<SdlContext>,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for RenderContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderContext")
            .field("size", &self.size())
            .field("textures", &self.textures.len())
            .field("camera", &self.camera)
            .field("batch", &self.batch)
            .finish_non_exhaustive()
    }
}

impl RenderContext {
    /// Creates the context for `window`, without vsync.
    pub fn new(window: Window) -> Result<RenderContext, GiochiError> {
        Self::with_vsync(window, false)
    }

    /// Creates the context for `window`, optionally waiting for vblank on
    /// [`RenderContext::present`].
    pub fn with_vsync(window: Window, vsync: bool) -> Result<RenderContext, GiochiError> {
        let (sdl_window, context) = window.into_parts();
        let canvas = build_canvas(sdl_window, vsync)?;
        let size = canvas.window().size();
        let textures = TextureRegistry::from_creator(canvas.texture_creator());

        Ok(RenderContext {
            canvas,
            textures: Rc::new(textures),
            resources: ResourceManager::new(),
            camera: Camera::new(Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32)),
            counters: FrameCounters::new(),
            batch: SpriteBatch::new(),
            window_size: size,
            _context: context,
            _reserved: [0; 4],
        })
    }

    /// Loads `paths` into one packed texture. See [`TextureAtlas::from_images`].
    ///
    /// Flushes the pending batch first: the atlas allocates a texture, and every
    /// queued sprite was submitted before that allocation.
    pub fn load_atlas(&mut self, paths: &[&str]) -> Result<TextureAtlas, GiochiError> {
        self.flush_batch();
        TextureAtlas::from_images(self, paths)
    }

    /// Sends every queued sprite to the backend, one call per texture run.
    ///
    /// Called automatically by [`RenderContext::present`], by any method that reads the
    /// target, and before an unbatched primitive. A host that wants to control when the
    /// work happens can call it explicitly; the cost is the same either way.
    pub fn flush_batch(&mut self) {
        if self.batch.is_empty() {
            return;
        }

        // Fields are borrowed separately instead of through `self`: the runs are an
        // iterator over `self.batch` now, and it stays alive for the whole loop while
        // the body needs `self.textures` and `self.canvas`.
        let Self {
            batch,
            textures,
            canvas,
            counters,
            ..
        } = self;

        for run in batch.flush_runs() {
            let handle = run.texture;
            let Some(texture) = textures.resolve(handle) else {
                // A texture destroyed with sprites still queued: those sprites were
                // submitted first, but there is nothing left to draw them with.
                log::debug!("batched draw skipped: texture {handle} is no longer alive");
                continue;
            };
            let result = match run.kind {
                // Nothing to merge in a run of one: the plain copy is cheaper than
                // building and submitting four vertices for a single sprite.
                RunKind::Single { dst, src } => match to_sdl_rect(dst) {
                    Some(dst) => canvas.copy(&texture, to_sdl_rect(src), dst),
                    // The run only exists because the quad had an area, so this is
                    // unreachable; treated as "nothing to draw" rather than a panic.
                    None => Ok(()),
                },
                RunKind::Geometry { vertices, indices } => {
                    canvas.render_geometry(vertices, Some(&texture), indices)
                }
            };
            match result {
                Ok(()) => {
                    #[cfg(feature = "profiling")]
                    counters.record_draw_calls(1);
                    #[cfg(not(feature = "profiling"))]
                    let _ = counters;
                }
                Err(err) => log::error!("batched draw failed: {err}"),
            }
        }

        self.batch.clear();
    }

    /// The batch, for inspection (`len`, `draw_calls`) and configuration.
    pub fn sprite_batch(&self) -> &SpriteBatch {
        &self.batch
    }

    /// Replaces the batching configuration, flushing what is queued first.
    pub fn set_batch_config(&mut self, config: SpriteBatchConfig) {
        self.flush_batch();
        self.batch = SpriteBatch::with_config(config);
    }

    /// The texture registry owned by this context.
    ///
    /// Note that holding this reference blocks `&mut self` calls (`clear`,
    /// `draw_*`, `present`), so prefer the delegating methods below in draw loops.
    pub fn textures(&self) -> &TextureRegistry {
        &self.textures
    }

    /// A shared handle to the texture registry.
    ///
    /// Every registry operation takes `&self`, so this clone is usable while the
    /// context itself is borrowed mutably. That is what the C ABI needs:
    /// `giochi_texture_create` must work from inside a scene callback, where the
    /// engine is already holding `&mut RenderContext` (see `docs/C_ABI.md`).
    pub fn shared_textures(&self) -> Rc<TextureRegistry> {
        Rc::clone(&self.textures)
    }

    /// Loads a texture and returns its handle. See [`TextureRegistry::create_texture`].
    ///
    /// This bypasses the resource cache: each call uploads a new texture. Use
    /// [`RenderContext::load_texture`] to share one upload per path.
    pub fn create_texture(&self, path: &str) -> Result<TextureHandle, GiochiError> {
        self.textures.create_texture(path)
    }

    /// Loads a texture through the resource cache.
    ///
    /// The second call for the same path returns the same handle and does not
    /// upload again, which is what a game with several entities sharing a sprite
    /// sheet wants.
    pub fn load_texture(&mut self, path: &str) -> Result<TextureHandle, GiochiError> {
        self.resources.load_texture(&self.textures, path)
    }

    /// Returns the cached handle for `path`, without loading.
    pub fn cached_texture(&self, path: &str) -> Option<TextureHandle> {
        self.resources.get_texture(&self.textures, path)
    }

    /// Number of paths the resource cache currently holds.
    pub fn cached_texture_count(&self) -> usize {
        self.resources.cached_texture_count()
    }

    /// The resource cache, for asset reads and cache inspection.
    pub fn resources(&self) -> &ResourceManager {
        &self.resources
    }

    /// The resource cache, mutably.
    pub fn resources_mut(&mut self) -> &mut ResourceManager {
        &mut self.resources
    }

    /// Drops every cached path and destroys the textures behind them.
    ///
    /// Lives here rather than only on [`ResourceManager`] because clearing the
    /// cache needs the registry at the same time, and a caller outside the crate
    /// cannot borrow both halves of a `RenderContext`.
    pub fn clear_resource_cache(&mut self) {
        self.flush_batch();
        self.resources.clear(&self.textures);
        debug_assert_eq!(self.resources.cached_texture_count(), 0);
    }

    /// Creates a texture from raw RGBA8 pixels.
    /// See [`TextureRegistry::create_texture_from_rgba`].
    pub fn create_texture_from_rgba(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<TextureHandle, GiochiError> {
        self.textures
            .create_texture_from_rgba(width, height, pixels)
    }

    /// Replaces the pixels of an RGBA texture in place.
    /// See [`TextureRegistry::update_texture_rgba`].
    pub(crate) fn update_texture_rgba(
        &self,
        texture: TextureHandle,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<(), GiochiError> {
        self.textures
            .update_texture_rgba(texture, width, height, pixels)
    }

    /// Releases a texture. Returns `false` for an invalid or already-freed handle.
    ///
    /// Flushes the pending batch first: queued sprites were submitted before the
    /// destroy and must not vanish because the handle went away.
    pub fn destroy_texture(&mut self, texture: TextureHandle) -> bool {
        self.flush_batch();
        self.textures.destroy_texture(texture)
    }

    /// Pixel size of a texture, or `None` for an invalid handle.
    pub fn texture_size(&self, texture: TextureHandle) -> Option<(u32, u32)> {
        self.textures.dimensions(texture)
    }

    /// The file `texture` was loaded from, or `None` for a generated one.
    pub fn texture_source(&self, texture: TextureHandle) -> Option<String> {
        self.textures.source_path(texture)
    }

    /// Reloads `texture` from the file it came from, keeping the same handle.
    ///
    /// This is what the hot reloader calls. It does **not** flush the batch: queued
    /// sprites hold handles, not pixels, so they draw the reloaded image on the frame
    /// that is already in flight — which is exactly the behaviour a developer wants.
    ///
    /// # Errors
    ///
    /// [`GiochiError::Asset`] when the file is gone or no longer decodes (the old
    /// texture is kept), [`GiochiError::InvalidArgument`] for a generated texture, and
    /// [`GiochiError::InvalidHandle`] for an unknown handle.
    pub fn reload_texture(&mut self, texture: TextureHandle) -> Result<(), GiochiError> {
        self.textures.reload(texture)
    }

    /// Reloads every live texture that was loaded from `path`.
    ///
    /// Returns how many reloaded. `0` means no texture came from that file, which is
    /// not an error: an edited asset that the running scene never loaded is the normal
    /// case.
    pub fn reload_texture_path(&mut self, path: &Path) -> usize {
        let handles: Vec<TextureHandle> = self
            .textures
            .handles()
            .into_iter()
            .filter(|handle| {
                self.textures
                    .source_path(*handle)
                    .is_some_and(|source| same_file(Path::new(&source), path))
            })
            .collect();

        let mut reloaded = 0usize;
        for handle in handles {
            match self.textures.reload(handle) {
                Ok(()) => {
                    reloaded += 1;
                    log::info!("reloaded texture {handle} from '{}'", path.display());
                }
                // A shared texture, or a file that no longer decodes: the old image
                // stays on screen and the reason goes to the log. A developer tool must
                // not take the game down.
                Err(err) => log::warn!("texture {handle} was not reloaded: {err}"),
            }
        }
        reloaded
    }

    /// Reloads the texture the resource cache holds for `path`, keeping its handle.
    ///
    /// This is the cache-aware reload: it asks [`ResourceManager`] which handle `path`
    /// maps to, so a game that loaded assets through [`RenderContext::load_texture`] can
    /// name a file and nothing else.
    ///
    /// # Errors
    ///
    /// [`GiochiError::InvalidArgument`] when the path is not cached, plus whatever the
    /// texture reload reports. A failed reload evicts the cache entry so the next load
    /// surfaces the real error.
    pub fn reload_cached_texture(&mut self, path: &str) -> Result<TextureHandle, GiochiError> {
        self.resources.reload_texture(&self.textures, path)
    }

    /// Number of live textures.
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// Name of the renderer backend the SDL actually opened.
    ///
    /// Asking is not the same as getting: an unknown `SDL_RENDER_DRIVER` falls
    /// back silently, and on an embedded board the difference is between the
    /// accelerated backend and the software one. Loading `libEGL` says nothing —
    /// the `kmsdrm` video driver uses EGL/GBM to present whatever the renderer
    /// is, so the only honest source is `SDL_GetRendererInfo`.
    pub fn renderer_name(&self) -> &'static str {
        self.canvas.info().name
    }

    /// Whether the backend reported hardware acceleration.
    pub fn renderer_is_accelerated(&self) -> bool {
        // `SDL_RENDERER_ACCELERATED`
        self.canvas.info().flags & 0x0000_0002 != 0
    }

    /// Drawable size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        self.canvas.window().size()
    }

    /// Size SDL renders into, in physical pixels (differs from [`Self::size`] on
    /// high-DPI displays).
    pub fn drawable_size(&self) -> (u32, u32) {
        self.canvas.window().drawable_size()
    }

    /// Replaces the window title.
    pub fn set_title(&mut self, title: &str) -> Result<(), GiochiError> {
        self.canvas
            .window_mut()
            .set_title(title)
            .map_err(|err| GiochiError::Platform(format!("invalid window title: {err}")))
    }

    /// Sets the camera used by [`RenderContext::draw_sprite`].
    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    /// Returns the camera currently in use.
    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// Clears the per-frame draw counters.
    ///
    /// The engine calls this once per frame, before the scene renders. Calling it is
    /// harmless without the `profiling` feature: the counters stay zero and the
    /// renderer's draw paths do not touch them.
    pub fn begin_frame_stats(&mut self) {
        #[cfg(feature = "profiling")]
        self.counters.reset();
    }

    /// Snapshot of the draw counters since [`RenderContext::begin_frame_stats`].
    ///
    /// Without the `profiling` feature every field is zero.
    pub fn frame_counters(&self) -> FrameCounters {
        self.counters
    }

    /// Fits the current camera's viewport to the window, keeping its position and
    /// zoom. Called at construction and available for resize handling.
    pub fn fit_camera_to_window(&mut self) {
        let (width, height) = self.size();
        self.window_size = (width, height);
        self.camera.viewport = Rect::new(0.0, 0.0, width as f32, height as f32);
    }

    /// Clears the whole target with `color`.
    ///
    /// Anything queued in the batch is drawn first, so a clear wipes what was
    /// submitted before it and nothing that comes after.
    pub fn clear(&mut self, color: Color) {
        self.flush_batch();
        self.window_size = self.size();
        self.canvas.set_draw_color(to_sdl_color(color));
        self.canvas.clear();
    }

    /// Fills `rect` (screen space) with `color`.
    ///
    /// Empty or degenerate rectangles are a no-op rather than an error.
    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        let Some(dst) = to_sdl_rect(rect) else {
            return;
        };
        self.flush_batch();
        self.canvas.set_draw_color(to_sdl_color(color));
        if let Err(err) = self.canvas.fill_rect(dst) {
            log::error!("draw_rect failed: {err}");
        }
        #[cfg(feature = "profiling")]
        self.counters.record_draw_call();
    }

    /// Draws the outline of `rect`, one pixel thick.
    ///
    /// `draw_rect` fills; a border drawn as four filled rectangles costs four
    /// calls per element and does not line up on the corners.
    pub fn stroke_rect(&mut self, rect: Rect, color: Color) {
        let Some(dst) = to_sdl_rect(rect) else {
            return;
        };
        self.flush_batch();
        self.canvas.set_draw_color(to_sdl_color(color));
        if let Err(err) = self.canvas.draw_rect(dst) {
            log::error!("stroke_rect failed: {err}");
        }
        #[cfg(feature = "profiling")]
        self.counters.record_draw_call();
    }

    /// Fills many rectangles with one colour in a single backend call.
    ///
    /// Rounded corners and circles are rasterised as 1-pixel-high spans, so one
    /// call per span would mean hundreds of backend round-trips per element.
    /// Empty rectangles are skipped by the backend.
    pub fn fill_rects(&mut self, rects: &[Rect], color: Color) {
        let converted: Vec<sdl2::rect::Rect> =
            rects.iter().filter_map(|r| to_sdl_rect(*r)).collect();
        if converted.is_empty() {
            return;
        }
        self.flush_batch();
        self.canvas.set_draw_color(to_sdl_color(color));
        if let Err(err) = self.canvas.fill_rects(&converted) {
            log::error!("fill_rects failed: {err}");
        }
        #[cfg(feature = "profiling")]
        self.counters.record_draw_call();
    }

    /// Draws a one-pixel line between two points.
    ///
    /// Icon paths and rounded corners are polylines; approximating them with
    /// 1-pixel rectangles serrates the diagonals twice (once by the rasteriser,
    /// once by the approximation).
    pub fn draw_line(&mut self, from: Vec2, to: Vec2, color: Color) {
        self.flush_batch();
        self.canvas.set_draw_color(to_sdl_color(color));
        // `to_sdl_coord`, not `as i32`, so this path follows the same policy as every other
        // draw in this module. Rust's `as` is saturating, so an infinite endpoint became
        // `i32::MAX`/`i32::MIN` and was handed to SDL's integer line clipping, where
        // extreme coordinates can overflow the C arithmetic; the guard maps a non-finite
        // value to 0 and clamps the rest to the representable range. `NaN` happens to map
        // to 0 either way, so this is about consistency and the infinite case, not about a
        // NaN silently moving the line.
        if let Err(err) = self.canvas.draw_line(
            (to_sdl_coord(from.x), to_sdl_coord(from.y)),
            (to_sdl_coord(to.x), to_sdl_coord(to.y)),
        ) {
            log::error!("draw_line failed: {err}");
        }
        #[cfg(feature = "profiling")]
        self.counters.record_draw_call();
    }

    /// Queues `src` of `texture` into `dst` (both screen space) for a batched draw.
    ///
    /// A zero-sized `src` means "the whole texture". An invalid handle is reported;
    /// an empty destination or one entirely outside the window is a no-op. Consecutive
    /// calls that share a texture become a single backend draw call at
    /// [`RenderContext::flush_batch`] time.
    pub fn draw_texture(
        &mut self,
        texture: TextureHandle,
        dst: Rect,
        src: Rect,
    ) -> Result<(), GiochiError> {
        let Some(size) = self.textures.size(texture) else {
            return Err(GiochiError::InvalidHandle(texture));
        };
        let Some(dst) = to_sdl_rect(dst) else {
            return Ok(());
        };

        if !self.is_visible(dst) {
            return Ok(());
        }
        if self.batch.is_full() {
            self.flush_batch();
        }

        self.batch
            .push_quad(texture, rect_from_sdl(dst), src, size)?;
        #[cfg(feature = "profiling")]
        self.counters.record_queued_sprite();
        Ok(())
    }

    /// Draws `texture` immediately, bypassing the batch.
    ///
    /// Kept for two callers: [`RenderContext::draw_texture_ex`], whose rotation and
    /// mirroring would need per-quad CPU geometry to batch, and the benchmark that
    /// compares batched and unbatched frames in the same binary. Normal drawing should
    /// use [`RenderContext::draw_texture`].
    pub fn draw_texture_immediate(
        &mut self,
        texture: TextureHandle,
        dst: Rect,
        src: Rect,
    ) -> Result<(), GiochiError> {
        let texture = self
            .textures
            .resolve(texture)
            .ok_or(GiochiError::InvalidHandle(texture))?;

        let Some(dst) = to_sdl_rect(dst) else {
            return Ok(());
        };
        let src = to_sdl_rect(src);

        // Anything queued was submitted before this draw, so it must land first.
        self.flush_batch();
        self.canvas
            .copy(&texture, src, dst)
            .map_err(|err| GiochiError::Render(format!("draw_texture failed: {err}")))?;
        #[cfg(feature = "profiling")]
        self.counters.record_sprite();
        Ok(())
    }

    /// Copies `src` into `dst` with rotation and optional mirroring.
    ///
    /// `angle_degrees` is clockwise, like SDL's `RenderCopyEx`. This is what
    /// makes a portrait panel possible: the UI is laid out in landscape and each
    /// element is rotated into device space, and a texture — text is a texture —
    /// has to rotate the same way. Rotating the *bitmap* in software instead
    /// would resample the glyphs and change their pixels.
    ///
    /// This path is not batched: SDL rotates around the centre of `dst`, and copying
    /// that semantics into four CPU-computed corners is not worth doing for the few
    /// rotated elements a 2D scene has. It flushes the pending batch first.
    pub fn draw_texture_ex(
        &mut self,
        texture: TextureHandle,
        src: Rect,
        dst: Rect,
        angle_degrees: f32,
        flip_horizontal: bool,
        flip_vertical: bool,
    ) -> Result<(), GiochiError> {
        let texture = self
            .textures
            .resolve(texture)
            .ok_or(GiochiError::InvalidHandle(texture))?;

        let Some(dst) = to_sdl_rect(dst) else {
            return Ok(());
        };
        let src = to_sdl_rect(src);

        self.flush_batch();
        self.canvas
            .copy_ex(
                &texture,
                src,
                dst,
                f64::from(angle_degrees),
                None,
                flip_horizontal,
                flip_vertical,
            )
            .map_err(|err| GiochiError::Render(format!("draw_texture_ex failed: {err}")))?;
        #[cfg(feature = "profiling")]
        self.counters.record_sprite();
        Ok(())
    }

    /// Draws `sprite`, converting its world-space rectangle through the camera.
    pub fn draw_sprite(&mut self, sprite: &Sprite) -> Result<(), GiochiError> {
        let dst = self.camera.world_rect_to_screen(sprite.world_rect());
        self.draw_texture(sprite.texture, dst, Rect::ZERO)
    }

    /// Converts a world-space point to screen space with the current camera.
    pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
        self.camera.world_to_screen(world)
    }

    /// Presents the back buffer on screen.
    ///
    /// Flushes the pending batch first, so every queued sprite is in the frame.
    pub fn present(&mut self) {
        self.flush_batch();
        self.canvas.present();
    }

    /// Reads back `rect` from the current target as RGBA8 bytes.
    ///
    /// Intended for tests and screenshot tooling: it forces a GPU→CPU copy and must
    /// not be used in a frame loop. Returns `row-major` pixels, 4 bytes each.
    ///
    /// Takes `&mut self` because a read has to flush the pending batch: returning the
    /// target without the sprites submitted before the read would be wrong.
    pub fn read_pixels(&mut self, rect: Rect) -> Result<Vec<u8>, GiochiError> {
        self.flush_batch();
        let Some(rect) = to_sdl_rect(rect) else {
            return Ok(Vec::new());
        };

        self.canvas
            .read_pixels(Some(rect), sdl2::pixels::PixelFormatEnum::RGBA32)
            .map_err(|err| GiochiError::Render(format!("read_pixels failed: {err}")))
    }

    /// Whether `dst` intersects the window.
    ///
    /// Sprites entirely outside it would be rasterised by the driver for nothing; the
    /// batch is the last place where skipping them is free. Partially visible sprites
    /// are kept whole — clipping happens in the renderer.
    fn is_visible(&self, dst: sdl2::rect::Rect) -> bool {
        let (width, height) = self.window_size;
        let left = dst.x();
        let top = dst.y();
        let right = left.saturating_add(dst.width() as i32);
        let bottom = top.saturating_add(dst.height() as i32);
        right > 0 && bottom > 0 && left < width as i32 && top < height as i32
    }

    pub(crate) fn texture_creator(&self) -> TextureCreator<WindowContext> {
        self.canvas.texture_creator()
    }
}

/// Whether `left` and `right` name the same file on disk.
///
/// The watcher reports canonical absolute paths while a texture was created from whatever
/// string the game passed — usually a relative asset path. Comparing the raw strings means
/// a reload never matches, so the comparison falls back to `canonicalize` when the cheap
/// equality fails. Both are files the process already opened, so a failure to canonicalise
/// is not an error worth reporting: the answer is simply "not the same file".
fn same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn build_canvas(window: SdlWindow, vsync: bool) -> Result<WindowCanvas, GiochiError> {
    let mut builder = window.into_canvas();
    if vsync {
        builder = builder.present_vsync();
    }

    let mut canvas = builder
        .build()
        .map_err(|err| GiochiError::Render(format!("failed to create the 2D renderer: {err}")))?;

    // O padrão do SDL é `BlendMode::None`, que **ignora o alfa**: um retângulo
    // `rgba(..., 30)` sai sólido. Texturas já nascem em `Blend` neste crate; o
    // alvo não nasce, e o resultado é uma UI com contornos e brilhos opacos.
    //
    // Medido ao portar o launcher: sem esta linha, 316 cores em vez de 321 e as
    // linhas de 12 % de opacidade viravam bordas duras.
    canvas.set_blend_mode(sdl2::render::BlendMode::Blend);
    Ok(canvas)
}

/// Converts an engine color into SDL's color type.
fn to_sdl_color(color: Color) -> sdl2::pixels::Color {
    sdl2::pixels::Color::RGBA(color.r, color.g, color.b, color.a)
}

/// Converts a float rectangle into SDL's integer rectangle.
///
/// Returns `None` when the rectangle has no area after rounding, which lets
/// callers treat "nothing to draw" as a no-op. Non-finite components become `0`,
/// and the result is clamped to the `i32` range SDL uses.
fn to_sdl_rect(rect: Rect) -> Option<sdl2::rect::Rect> {
    let x = to_sdl_coord(rect.x);
    let y = to_sdl_coord(rect.y);
    let w = to_sdl_extent(rect.w);
    let h = to_sdl_extent(rect.h);

    if w <= 0 || h <= 0 {
        return None;
    }

    Some(sdl2::rect::Rect::new(x, y, w as u32, h as u32))
}

/// Converts SDL's integer rectangle back into the engine's float rectangle.
///
/// Used by the batcher, which receives the already-rounded destination so the vertices
/// land on the same pixel grid SDL's own `copy` would have used.
fn rect_from_sdl(rect: sdl2::rect::Rect) -> Rect {
    Rect::new(
        rect.x() as f32,
        rect.y() as f32,
        rect.width() as f32,
        rect.height() as f32,
    )
}

fn to_sdl_coord(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    value.round().clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

fn to_sdl_extent(value: f32) -> i32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    value.round().clamp(0.0, i32::MAX as f32) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_map_channel_by_channel() {
        let color = to_sdl_color(Color::new(1, 2, 3, 4));

        assert_eq!((color.r, color.g, color.b, color.a), (1, 2, 3, 4));
    }

    #[test]
    fn rects_are_rounded_to_integers() {
        let rect = to_sdl_rect(Rect::new(1.4, 2.6, 10.5, 20.4)).expect("non-empty rect");

        assert_eq!((rect.x(), rect.y(), rect.w, rect.h), (1, 3, 11, 20));
    }

    #[test]
    fn empty_and_degenerate_rects_are_skipped() {
        for rect in [
            Rect::new(0.0, 0.0, 0.0, 10.0),
            Rect::new(0.0, 0.0, 10.0, 0.0),
            Rect::new(0.0, 0.0, -5.0, 10.0),
            Rect::new(0.0, 0.0, 0.4, 0.4),
        ] {
            assert!(to_sdl_rect(rect).is_none(), "{rect:?} must be skipped");
        }
    }

    #[test]
    fn non_finite_rects_do_not_panic() {
        let rect = to_sdl_rect(Rect::new(f32::NAN, f32::INFINITY, 10.0, 10.0))
            .expect("finite extents survive");

        assert_eq!((rect.x(), rect.y()), (0, 0));
        assert_eq!((rect.w, rect.h), (10, 10));
    }

    #[test]
    fn huge_coordinates_are_clamped_instead_of_wrapping() {
        let rect = to_sdl_rect(Rect::new(1e30, -1e30, 1e30, 1e30)).expect("clamped rect");

        assert!(rect.x() > 0);
        assert!(rect.y() < 0);
    }
}
