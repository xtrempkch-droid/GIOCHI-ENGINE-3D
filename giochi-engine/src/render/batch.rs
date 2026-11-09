//! Sprite batching for the 2D renderer.
//!
//! A [`SpriteBatch`] accumulates textured quads and turns them into as few backend
//! draw calls as the submission order allows: every run of consecutive quads that share
//! a texture becomes one `SDL_RenderGeometry` call.
//!
//! # Why consecutive runs, not "sort by texture"
//!
//! SDL's 2D renderer has no depth buffer: overlapping draws composite in submission
//! order. Reordering quads by texture would therefore change what a game looks like,
//! silently, whenever two sprites overlap. The batch keeps the order and only merges
//! neighbours, which is why a scene that draws its sprites grouped by texture (a
//! tilemap, then entities, or everything from one atlas) collapses to one call, while a
//! 2D z-order is still exactly what the caller submitted. See ADR-043.
//!
//! # Measurement
//!
//! `docs/PERF_LOG.md` records the phase-16 numbers: 1000 sprites of one texture go from
//! 1000 backend calls to 1.

use sdl2::pixels::Color as SdlColor;
use sdl2::rect::FPoint;
use sdl2::render::Vertex;

use crate::error::GiochiError;
use crate::math::Rect;
use crate::render::texture::{TextureHandle, TextureSize};

/// Default number of sprites a batch holds before it asks to be flushed.
///
/// Not larger on purpose. A single `SDL_RenderGeometry` call of several thousand
/// quads costs more than the same quads split into chunks on the software renderer,
/// which sets up the vertex array per call but then rasterises each triangle with
/// per-vertex interpolation instead of the fast blit path `RenderCopy` uses. Measured
/// on the `software` renderer at 5000 sprites: 1024 quads per call 28.5 ms, 4096
/// quads per call 44.9 ms; on `opengl`/`opengles2` the limit makes no measurable
/// difference. See `docs/PERF_LOG.md`.
pub const DEFAULT_MAX_SPRITES: u32 = 1024;

/// Default number of sprites' worth of vertex capacity allocated up front.
pub const DEFAULT_INITIAL_SPRITES: u32 = 256;

/// Vertex colour used for every batched quad: white multiplies the texture unchanged.
const VERTEX_COLOR: SdlColor = SdlColor::RGBA(255, 255, 255, 255);

/// Configuration of a [`SpriteBatch`].
///
/// # C ABI
///
/// Not exposed; batching is an implementation detail of `giochi_render_draw_texture`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteBatchConfig {
    /// Sprites whose vertices are allocated up front. `0` uses the default.
    pub initial_sprites: u32,
    /// Sprites queued before the caller must flush. `0` uses the default.
    pub max_sprites: u32,
    /// Reserved for future ABI-compatible fields. Must be zero.
    pub _reserved: [u32; 4],
}

impl SpriteBatchConfig {
    /// Creates a configuration with explicit limits.
    pub const fn new(initial_sprites: u32, max_sprites: u32) -> Self {
        Self {
            initial_sprites,
            max_sprites,
            _reserved: [0; 4],
        }
    }

    /// The initial capacity actually used, defaulting when it is zero or larger than
    /// the maximum.
    pub fn resolved_initial_sprites(&self) -> u32 {
        let max = self.resolved_max_sprites();
        match self.initial_sprites {
            0 => DEFAULT_INITIAL_SPRITES.min(max),
            value => value.min(max),
        }
    }

    /// The batch limit actually used, defaulting when it is zero.
    pub fn resolved_max_sprites(&self) -> u32 {
        if self.max_sprites == 0 {
            DEFAULT_MAX_SPRITES
        } else {
            self.max_sprites
        }
    }
}

impl Default for SpriteBatchConfig {
    fn default() -> Self {
        Self::new(DEFAULT_INITIAL_SPRITES, DEFAULT_MAX_SPRITES)
    }
}

/// One run of consecutive quads that share a texture.
///
/// Runs are contiguous slices of the batch's vertex and index buffers, so flushing in
/// run order reproduces the submission order exactly.
#[derive(Debug, Clone, Copy)]
struct Run {
    texture: TextureHandle,
    first_vertex: u32,
    vertex_count: u32,
    first_index: u32,
    index_count: u32,
    /// The quad's rectangles, kept only while the run holds exactly one. A run of one
    /// has nothing to merge, and a backend copy of one quad is cheaper than building
    /// and submitting four vertices — measured at ~4 % on the software renderer.
    single: Option<(Rect, Rect)>,
}

/// How [`crate::render::RenderContext::flush_batch`] should draw one run.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RunKind<'a> {
    /// One quad that stayed alone: draw it with a plain texture copy.
    Single {
        /// Destination rectangle.
        dst: Rect,
        /// Source rectangle in texture pixels; empty means the whole texture.
        src: Rect,
    },
    /// Several quads sharing a texture: draw them as one geometry call.
    Geometry {
        /// Vertices of the run.
        vertices: &'a [Vertex],
        /// Indices of the run, relative to its first vertex.
        indices: &'a [i32],
    },
}

/// One run with the data the renderer needs to draw it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunView<'a> {
    /// Texture of every quad in the run.
    pub(crate) texture: TextureHandle,
    /// How to draw it.
    pub(crate) kind: RunKind<'a>,
}

/// Accumulates textured quads and splits them into per-texture runs.
///
/// The batch itself does not touch SDL: it owns vertex data and texture handles, and
/// [`crate::render::RenderContext::flush_batch`] hands each run to the canvas. That
/// split is what makes the geometry unit-testable without a window.
///
/// # C ABI
///
/// Not exposed. [`crate::render::RenderContext`] owns one and C consumers keep calling
/// `giochi_render_draw_texture`, which batches transparently (R12).
#[repr(C)]
pub struct SpriteBatch {
    vertices: Vec<Vertex>,
    indices: Vec<i32>,
    runs: Vec<Run>,
    config: SpriteBatchConfig,
    sprites: u32,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for SpriteBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpriteBatch")
            .field("sprites", &self.sprites)
            .field("runs", &self.runs.len())
            .field("vertices", &self.vertices.len())
            .field("max_sprites", &self.config.resolved_max_sprites())
            .finish_non_exhaustive()
    }
}

impl SpriteBatch {
    /// Creates an empty batch with the default limits.
    pub fn new() -> Self {
        Self::with_config(SpriteBatchConfig::default())
    }

    /// Creates an empty batch with explicit limits.
    pub fn with_config(config: SpriteBatchConfig) -> Self {
        let initial = config.resolved_initial_sprites() as usize;
        Self {
            vertices: Vec::with_capacity(initial * 4),
            indices: Vec::with_capacity(initial * 6),
            runs: Vec::new(),
            config,
            sprites: 0,
            _reserved: [0; 4],
        }
    }

    /// The batch's configuration.
    pub fn config(&self) -> SpriteBatchConfig {
        self.config
    }

    /// Replaces the configuration. Drops whatever was queued.
    pub fn set_config(&mut self, config: SpriteBatchConfig) {
        *self = Self::with_config(config);
    }

    /// Number of quads queued since the last flush.
    pub fn len(&self) -> u32 {
        self.sprites
    }

    /// Whether nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.sprites == 0
    }

    /// Whether the batch reached its limit and should be flushed before the next push.
    pub fn is_full(&self) -> bool {
        self.sprites >= self.config.resolved_max_sprites()
    }

    /// Number of backend draw calls a flush would issue right now.
    pub fn draw_calls(&self) -> u32 {
        self.runs.len() as u32
    }

    /// Number of vertices currently queued.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Number of indices currently queued.
    pub fn index_count(&self) -> usize {
        self.indices.len()
    }

    /// Texture of every run, in submission order.
    ///
    /// Runs are contiguous in the vertex and index buffers, so a caller that wants to
    /// draw them itself can walk the buffers; this exists for inspection and tests.
    pub fn run_textures(&self) -> Vec<TextureHandle> {
        self.runs.iter().map(|run| run.texture).collect()
    }

    /// Queues one textured quad.
    ///
    /// `src` is in texture pixels; an empty `src` means the whole texture. Both `dst`
    /// and `src` are clipped: a sprite partly outside the texture draws the visible
    /// part, and a sprite with no visible area is skipped. Returns [`Ok`] in both
    /// cases — "nothing to draw" is not an error.
    ///
    /// A texture change starts a new run; consecutive quads of the same texture share
    /// one.
    pub fn push_quad(
        &mut self,
        texture: TextureHandle,
        dst: Rect,
        src: Rect,
        texture_size: TextureSize,
    ) -> Result<(), GiochiError> {
        let Some(quad) = Quad::new(dst, src, texture_size) else {
            return Ok(());
        };

        // Indices are relative to the run: SDL validates them against the vertex slice
        // it is given, and every run is drawn as its own `render_geometry` call.
        if self.runs.last().map(|run| run.texture) != Some(texture) {
            self.runs.push(Run {
                texture,
                first_vertex: self.vertices.len() as u32,
                vertex_count: 0,
                first_index: self.indices.len() as u32,
                index_count: 0,
                single: Some((dst, src)),
            });
        }

        let run = self.runs.last_mut().expect("a run was just ensured");
        let base = run.vertex_count;
        self.vertices.extend_from_slice(&quad.vertices);
        self.indices.extend_from_slice(&quad.indices(base));
        if run.vertex_count > 0 {
            run.single = None;
        }
        run.vertex_count += 4;
        run.index_count += 6;
        self.sprites = self.sprites.saturating_add(1);

        Ok(())
    }

    /// Empties the batch without drawing.
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.runs.clear();
        self.sprites = 0;
    }

    /// Vertex buffer of the queued quads, for tests that assert on geometry.
    #[cfg(test)]
    fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    /// Index buffer of the queued quads, for tests that assert on geometry.
    #[cfg(test)]
    fn indices(&self) -> &[i32] {
        &self.indices
    }

    /// Every queued run, in submission order, with the data needed to draw it.
    pub(crate) fn flush_runs(&self) -> Vec<RunView<'_>> {
        self.runs
            .iter()
            .map(|run| {
                let kind = match run.single {
                    Some((dst, src)) => RunKind::Single { dst, src },
                    None => RunKind::Geometry {
                        vertices: &self.vertices[run.first_vertex as usize
                            ..(run.first_vertex + run.vertex_count) as usize],
                        indices: &self.indices[run.first_index as usize
                            ..(run.first_index + run.index_count) as usize],
                    },
                };
                RunView {
                    texture: run.texture,
                    kind,
                }
            })
            .collect()
    }
}

impl Default for SpriteBatch {
    fn default() -> Self {
        Self::new()
    }
}

/// The four vertices and six indices of one quad, in screen space.
struct Quad {
    vertices: [Vertex; 4],
}

impl Quad {
    fn new(dst: Rect, src: Rect, texture_size: TextureSize) -> Option<Self> {
        let (dst, src) = sanitise(dst, src)?;
        let (u0, v0, u1, v1) = uv_rect(src, texture_size)?;

        // A source rectangle that reaches outside the texture shrinks the destination
        // by the same fraction, which is what `SDL_RenderCopy` does. Without this a
        // single-quad run (drawn with `copy`) and a multi-quad run (drawn from these
        // vertices) would disagree for the same call.
        let (dst, _, _) = shrink_destination_to_source(dst, src, texture_size);
        let (left, top, right, bottom) = (dst.x, dst.y, dst.x + dst.w, dst.y + dst.h);

        let vertex = |x: f32, y: f32, u: f32, v: f32| Vertex {
            position: FPoint::new(x, y),
            color: VERTEX_COLOR,
            tex_coord: FPoint::new(u, v),
        };

        Some(Self {
            vertices: [
                vertex(left, top, u0, v0),
                vertex(right, top, u1, v0),
                vertex(right, bottom, u1, v1),
                vertex(left, bottom, u0, v1),
            ],
        })
    }

    /// Indices for this quad, relative to the run's first vertex.
    fn indices(&self, base: u32) -> [i32; 6] {
        const PATTERN: [u32; 6] = [0, 1, 2, 0, 2, 3];
        PATTERN.map(|offset| (base + offset) as i32)
    }
}

/// Shrinks `dst` to the part of `src` that actually lies inside the texture.
///
/// Returns the destination, the source rectangle clipped to the texture, and the
/// source pixel size. An empty `src` means the whole texture, which is always inside
/// it and needs no shrinking.
fn shrink_destination_to_source(
    dst: Rect,
    src: Rect,
    texture_size: TextureSize,
) -> (Rect, Rect, (f32, f32)) {
    let (width, height) = (texture_size.width as f32, texture_size.height as f32);
    let src = if src.is_empty() {
        Rect::new(0.0, 0.0, width, height)
    } else {
        src
    };
    if src.w <= 0.0 || src.h <= 0.0 {
        return (dst, src, (width, height));
    }

    let left = src.x.max(0.0);
    let top = src.y.max(0.0);
    let right = (src.x + src.w).min(width);
    let bottom = (src.y + src.h).min(height);
    if right <= left || bottom <= top {
        return (Rect::new(dst.x, dst.y, 0.0, 0.0), src, (width, height));
    }
    if left == src.x && top == src.y && right == src.x + src.w && bottom == src.y + src.h {
        return (dst, src, (width, height));
    }

    let scale_x = dst.w / src.w;
    let scale_y = dst.h / src.h;
    let shrunk = Rect::new(
        dst.x + (left - src.x) * scale_x,
        dst.y + (top - src.y) * scale_y,
        (right - left) * scale_x,
        (bottom - top) * scale_y,
    );
    (
        shrunk,
        Rect::new(left, top, right - left, bottom - top),
        (width, height),
    )
}

/// Rejects a quad whose destination has no area and sanitises both rectangles.
fn sanitise(dst: Rect, src: Rect) -> Option<(Rect, Rect)> {
    let finite = |value: f32| value.is_finite();
    if !(finite(dst.x) && finite(dst.y) && finite(dst.w) && finite(dst.h)) {
        return None;
    }
    if dst.w <= 0.0 || dst.h <= 0.0 {
        return None;
    }
    if src.is_empty() {
        return Some((dst, Rect::ZERO));
    }
    if !(finite(src.x) && finite(src.y) && finite(src.w) && finite(src.h)) {
        return None;
    }
    if src.w <= 0.0 || src.h <= 0.0 {
        return None;
    }
    Some((dst, src))
}

/// Normalised UV rectangle for `src`, clipped to the texture.
///
/// Returns `None` for a texture with no size, or for a source rectangle entirely
/// outside it.
fn uv_rect(src: Rect, texture_size: TextureSize) -> Option<(f32, f32, f32, f32)> {
    let width = texture_size.width as f32;
    let height = texture_size.height as f32;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    let src = if src.is_empty() {
        Rect::new(0.0, 0.0, width, height)
    } else {
        src
    };

    let left = src.x.max(0.0);
    let top = src.y.max(0.0);
    let right = (src.x + src.w).min(width);
    let bottom = (src.y + src.h).min(height);
    if right <= left || bottom <= top {
        return None;
    }

    Some((left / width, top / height, right / width, bottom / height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(width: u32, height: u32) -> TextureSize {
        TextureSize::new(width, height)
    }

    #[test]
    fn a_new_batch_is_empty_and_zeroes_its_reserved_tail() {
        let batch = SpriteBatch::new();

        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
        assert_eq!(batch.draw_calls(), 0);
        assert_eq!(batch.vertex_count(), 0);
        assert_eq!(batch.index_count(), 0);
        assert_eq!(batch._reserved, [0; 4]);
    }

    #[test]
    fn one_quad_is_four_vertices_six_indices_and_one_draw_call() {
        let mut batch = SpriteBatch::new();

        batch
            .push_quad(
                1,
                Rect::new(10.0, 20.0, 30.0, 40.0),
                Rect::ZERO,
                size(64, 64),
            )
            .expect("push");

        assert_eq!(batch.len(), 1);
        assert_eq!(batch.vertex_count(), 4);
        assert_eq!(batch.index_count(), 6);
        assert_eq!(batch.draw_calls(), 1);
        assert_eq!(batch.run_textures(), vec![1]);
        assert_eq!(batch.indices(), &[0, 1, 2, 0, 2, 3]);
        assert_eq!(batch.vertices()[0].position.x, 10.0);
        assert_eq!(batch.vertices()[0].position.y, 20.0);
        assert_eq!(batch.vertices()[2].position.x, 40.0);
        assert_eq!(batch.vertices()[2].position.y, 60.0);
    }

    #[test]
    fn the_whole_texture_is_the_uv_unit_square() {
        let mut batch = SpriteBatch::new();
        batch
            .push_quad(1, Rect::new(0.0, 0.0, 8.0, 8.0), Rect::ZERO, size(16, 32))
            .expect("push");

        let vertices = batch.vertices();
        assert_eq!(
            (vertices[0].tex_coord.x, vertices[0].tex_coord.y),
            (0.0, 0.0)
        );
        assert_eq!(
            (vertices[2].tex_coord.x, vertices[2].tex_coord.y),
            (1.0, 1.0)
        );
    }

    #[test]
    fn a_sub_rectangle_maps_to_its_uv_window() {
        let mut batch = SpriteBatch::new();
        batch
            .push_quad(
                1,
                Rect::new(0.0, 0.0, 8.0, 8.0),
                Rect::new(8.0, 16.0, 8.0, 8.0),
                size(32, 32),
            )
            .expect("push");

        let vertices = batch.vertices();
        assert!((vertices[0].tex_coord.x - 0.25).abs() < 1e-6);
        assert!((vertices[0].tex_coord.y - 0.5).abs() < 1e-6);
        assert!((vertices[2].tex_coord.x - 0.5).abs() < 1e-6);
        assert!((vertices[2].tex_coord.y - 0.75).abs() < 1e-6);
    }

    #[test]
    fn a_source_partly_outside_the_texture_is_clipped_not_wrapped() {
        let mut batch = SpriteBatch::new();
        // 8 pixels requested, only the last 4 exist.
        batch
            .push_quad(
                1,
                Rect::new(0.0, 0.0, 8.0, 8.0),
                Rect::new(-4.0, 0.0, 8.0, 8.0),
                size(16, 16),
            )
            .expect("push");

        let vertices = batch.vertices();
        assert_eq!(vertices[0].tex_coord.x, 0.0);
        assert!((vertices[2].tex_coord.x - 0.25).abs() < 1e-6);
    }

    #[test]
    fn consecutive_quads_with_one_texture_share_a_draw_call() {
        let mut batch = SpriteBatch::new();
        for index in 0..100 {
            batch
                .push_quad(
                    7,
                    Rect::new(index as f32, 0.0, 4.0, 4.0),
                    Rect::ZERO,
                    size(8, 8),
                )
                .expect("push");
        }

        assert_eq!(batch.len(), 100);
        assert_eq!(batch.draw_calls(), 1);
        assert_eq!(batch.vertex_count(), 400);
        assert_eq!(batch.index_count(), 600);
    }

    #[test]
    fn a_texture_change_starts_a_new_run_and_keeps_the_order() {
        let mut batch = SpriteBatch::new();
        for texture in [1u64, 1, 2, 2, 1] {
            batch
                .push_quad(
                    texture,
                    Rect::new(0.0, 0.0, 4.0, 4.0),
                    Rect::ZERO,
                    size(8, 8),
                )
                .expect("push");
        }

        // 1, 1 | 2, 2 | 1 -> three runs, in that order.
        assert_eq!(batch.draw_calls(), 3);
        assert_eq!(batch.run_textures(), vec![1, 2, 1]);
    }

    #[test]
    fn a_degenerate_destination_is_skipped() {
        let mut batch = SpriteBatch::new();

        for dst in [
            Rect::new(0.0, 0.0, 0.0, 10.0),
            Rect::new(0.0, 0.0, 10.0, 0.0),
            Rect::new(f32::NAN, 0.0, 10.0, 10.0),
            Rect::new(0.0, 0.0, f32::INFINITY, 10.0),
        ] {
            batch
                .push_quad(1, dst, Rect::ZERO, size(8, 8))
                .expect("push");
        }

        assert!(batch.is_empty());
        assert_eq!(batch.draw_calls(), 0);
    }

    #[test]
    fn a_source_outside_the_texture_is_skipped() {
        let mut batch = SpriteBatch::new();

        assert!(batch
            .push_quad(
                1,
                Rect::new(0.0, 0.0, 4.0, 4.0),
                Rect::new(100.0, 0.0, 4.0, 4.0),
                size(8, 8)
            )
            .is_ok());
        assert!(batch.is_empty());
    }

    #[test]
    fn a_source_reaching_past_the_texture_shrinks_the_destination() {
        // SDL's `RenderCopy` clips the source to the texture and shrinks the
        // destination by the same fraction. The geometry path has to agree, because a
        // run of one quad is drawn with `copy` and a run of two from these vertices.
        let (dst, src, size_px) = shrink_destination_to_source(
            Rect::new(10.0, 20.0, 16.0, 16.0),
            Rect::new(-4.0, 0.0, 8.0, 8.0),
            size(8, 8),
        );

        assert_eq!(size_px, (8.0, 8.0));
        assert_eq!(
            src,
            Rect::new(0.0, 0.0, 4.0, 8.0),
            "half the source is outside"
        );
        assert_eq!(
            dst,
            Rect::new(18.0, 20.0, 8.0, 16.0),
            "the destination loses the same half"
        );
    }

    #[test]
    fn a_source_inside_the_texture_is_left_alone() {
        let dst = Rect::new(1.0, 2.0, 8.0, 8.0);
        let src = Rect::new(0.0, 0.0, 4.0, 4.0);
        assert_eq!(
            shrink_destination_to_source(dst, src, size(8, 8)).0,
            dst,
            "no clipping means no change"
        );
        assert_eq!(
            shrink_destination_to_source(dst, Rect::ZERO, size(8, 8)).0,
            dst,
            "the whole texture is always inside"
        );
    }

    #[test]
    fn a_zero_sized_texture_draws_nothing() {
        let mut batch = SpriteBatch::new();

        assert!(batch
            .push_quad(1, Rect::new(0.0, 0.0, 4.0, 4.0), Rect::ZERO, size(0, 0))
            .is_ok());
        assert!(batch.is_empty());
    }

    #[test]
    fn the_batch_reports_when_it_is_full_and_can_be_cleared() {
        let mut batch = SpriteBatch::with_config(SpriteBatchConfig::new(1, 2));
        assert!(!batch.is_full());

        batch
            .push_quad(1, Rect::new(0.0, 0.0, 4.0, 4.0), Rect::ZERO, size(8, 8))
            .expect("push");
        assert!(!batch.is_full());
        batch
            .push_quad(1, Rect::new(0.0, 0.0, 4.0, 4.0), Rect::ZERO, size(8, 8))
            .expect("push");
        assert!(batch.is_full());

        batch.clear();
        assert!(batch.is_empty());
        assert_eq!(batch.draw_calls(), 0);
    }

    #[test]
    fn the_configuration_defaults_zero_values() {
        let config = SpriteBatchConfig::new(0, 0);

        assert_eq!(config.resolved_max_sprites(), DEFAULT_MAX_SPRITES);
        assert_eq!(
            config.resolved_initial_sprites(),
            DEFAULT_INITIAL_SPRITES.min(DEFAULT_MAX_SPRITES)
        );
        assert_eq!(
            SpriteBatchConfig::new(99, 10).resolved_initial_sprites(),
            10
        );
        assert_eq!(SpriteBatchConfig::default()._reserved, [0; 4]);
    }

    #[test]
    fn flush_runs_covers_every_vertex_and_index_once() {
        let mut batch = SpriteBatch::new();
        for texture in [3u64, 3, 4] {
            batch
                .push_quad(
                    texture,
                    Rect::new(0.0, 0.0, 4.0, 4.0),
                    Rect::ZERO,
                    size(8, 8),
                )
                .expect("push");
        }

        let runs = batch.flush_runs();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].texture, 3);
        let RunKind::Geometry { vertices, indices } = runs[0].kind else {
            panic!("two quads share a texture, so the run is geometry");
        };
        assert_eq!(vertices.len(), 8);
        assert_eq!(indices.len(), 12);
        assert_eq!(
            indices,
            &[0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            "the second quad of a run follows the first, relative to the run"
        );

        assert_eq!(runs[1].texture, 4);
        let RunKind::Single { dst, src } = runs[1].kind else {
            panic!("a run of one quad keeps its rectangles");
        };
        assert_eq!(dst, Rect::new(0.0, 0.0, 4.0, 4.0));
        assert!(src.is_empty(), "an empty source means the whole texture");
    }

    #[test]
    fn a_single_quad_run_keeps_its_source_rectangle() {
        let mut batch = SpriteBatch::new();
        let src = Rect::new(8.0, 16.0, 4.0, 4.0);
        batch
            .push_quad(7, Rect::new(1.0, 2.0, 4.0, 4.0), src, size(64, 64))
            .expect("push");

        let runs = batch.flush_runs();
        assert_eq!(runs.len(), 1);
        let RunKind::Single { dst, src: kept } = runs[0].kind else {
            panic!("one quad is a single run");
        };
        assert_eq!(dst, Rect::new(1.0, 2.0, 4.0, 4.0));
        assert_eq!(kept, src, "the original source rectangle is preserved");
    }

    #[test]
    fn a_second_quad_turns_a_single_run_into_geometry() {
        let mut batch = SpriteBatch::new();
        batch
            .push_quad(1, Rect::new(0.0, 0.0, 4.0, 4.0), Rect::ZERO, size(8, 8))
            .expect("push");
        batch
            .push_quad(1, Rect::new(4.0, 0.0, 4.0, 4.0), Rect::ZERO, size(8, 8))
            .expect("push");

        let runs = batch.flush_runs();
        assert_eq!(runs.len(), 1);
        assert!(matches!(runs[0].kind, RunKind::Geometry { .. }));
    }
}
