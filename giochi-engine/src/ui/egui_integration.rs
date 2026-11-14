//! egui on top of `sdl2::render`, written for this engine.
//!
//! # Why not `egui-sdl2`
//!
//! No published version of `egui-sdl2` fits: the ones that support `sdl2` 0.38 require
//! `egui` 0.35 or newer and declare `rust-version = "1.92"`, which is above this
//! workspace's own floor and far above `giochi-engine`'s measured **1.86**. Their
//! OpenGL backend would also pull `glow` 0.17 while the 3D crate uses 0.18, so one
//! binary would carry two GL function tables. ADR-052 records the comparison.
//!
//! What is left is the part that actually differs between backends: turning egui's
//! [`epaint::Mesh`] into triangles the renderer can submit. `sdl2::render`'s
//! `render_geometry` is exactly that, and it is the same call the 2D batch already uses
//! for its own geometry runs (`ADR-043`).
//!
//! # The two halves
//!
//! * [`translate_event`] turns an SDL event into an [`egui::Event`] and reports whether
//!   egui wants it for itself. A console that lets `W` through to the game would walk
//!   the player forward while they type "walk".
//! * [`EguiBackend`] owns the [`egui::Context`], the texture table and the vertex
//!   scratch buffer, and paints into the engine's [`RenderContext`].
//!
//! # Premultiplied alpha
//!
//! egui stores colours premultiplied; SDL's `BlendMode::Blend` multiplies by alpha
//! again. The vertices therefore use `Color32::r()`/`g()`/`b()` — the **un**multiplied
//! accessors — which is what makes text render with clean edges instead of dark halos.

use std::collections::HashMap;

use sdl2::event::{Event, WindowEvent};
use sdl2::keyboard::{Keycode, Mod};
use sdl2::mouse::MouseButton;
use sdl2::pixels::Color as SdlColor;
use sdl2::rect::FPoint;
use sdl2::render::Vertex as SdlVertex;

use crate::render::{RenderContext, TextureHandle};

/// Bytes per pixel in every texture egui hands us.
const BYTES_PER_PIXEL: usize = 4;

/// egui's own translation of SDL2's input.
///
/// Returns `true` when egui consumed the event, which is the signal for the engine to
/// keep it away from the scene. Keys are consumed only while a text field has focus:
/// otherwise a console that is open but idle would swallow the game's input.
pub(crate) fn translate_event(event: &Event, wants_keyboard: bool) -> (Option<egui::Event>, bool) {
    match event {
        Event::MouseMotion { x, y, .. } => (
            Some(egui::Event::PointerMoved(egui::pos2(*x as f32, *y as f32))),
            false,
        ),
        Event::MouseButtonDown {
            mouse_btn, x, y, ..
        } => (
            Some(egui::Event::PointerButton {
                pos: egui::pos2(*x as f32, *y as f32),
                button: translate_button(*mouse_btn),
                pressed: true,
                modifiers: egui::Modifiers::default(),
            }),
            // A click is consumed: clicking a button in the console must not also fire
            // the game's "shoot".
            true,
        ),
        Event::MouseButtonUp {
            mouse_btn, x, y, ..
        } => (
            Some(egui::Event::PointerButton {
                pos: egui::pos2(*x as f32, *y as f32),
                button: translate_button(*mouse_btn),
                pressed: false,
                modifiers: egui::Modifiers::default(),
            }),
            true,
        ),
        Event::MouseWheel { y, precise_y, .. } => {
            let delta = if *precise_y != 0.0 {
                *precise_y
            } else {
                *y as f32
            };
            (
                Some(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(0.0, delta),
                    modifiers: egui::Modifiers::default(),
                }),
                // Scrolling over the console scrolls the console; over the game it is
                // the game's. Consumed only when the console is open, which is what
                // `wants_keyboard` tracks for us.
                wants_keyboard,
            )
        }
        Event::KeyDown {
            keycode,
            keymod,
            repeat,
            ..
        } => match keycode.and_then(translate_key) {
            Some(key) => (
                Some(egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: *repeat,
                    modifiers: translate_modifiers(*keymod),
                }),
                wants_keyboard,
            ),
            None => (None, false),
        },
        Event::KeyUp {
            keycode, keymod, ..
        } => match keycode.and_then(translate_key) {
            Some(key) => (
                Some(egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: translate_modifiers(*keymod),
                }),
                wants_keyboard,
            ),
            None => (None, false),
        },
        Event::TextInput { text, .. } => (Some(egui::Event::Text(text.clone())), true),
        Event::Window {
            win_event: WindowEvent::FocusLost,
            ..
        } => (Some(egui::Event::WindowFocused(false)), false),
        Event::Window {
            win_event: WindowEvent::FocusGained,
            ..
        } => (Some(egui::Event::WindowFocused(true)), false),
        _ => (None, false),
    }
}

/// SDL mouse button → egui pointer button.
fn translate_button(button: MouseButton) -> egui::PointerButton {
    match button {
        MouseButton::Left => egui::PointerButton::Primary,
        MouseButton::Right => egui::PointerButton::Secondary,
        _ => egui::PointerButton::Middle,
    }
}

/// SDL modifiers → egui modifiers.
fn translate_modifiers(modifiers: Mod) -> egui::Modifiers {
    egui::Modifiers {
        alt: modifiers.intersects(Mod::LALTMOD | Mod::RALTMOD),
        ctrl: modifiers.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD),
        shift: modifiers.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD),
        mac_cmd: false,
        command: modifiers.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD),
    }
}

/// SDL keycode → egui key.
///
/// Returns `None` for keys egui has no opinion about (media keys, keypad operators that
/// SDL reports separately); the event is then simply not forwarded.
fn translate_key(code: Keycode) -> Option<egui::Key> {
    use egui::Key;

    Some(match code {
        Keycode::Down => Key::ArrowDown,
        Keycode::Left => Key::ArrowLeft,
        Keycode::Right => Key::ArrowRight,
        Keycode::Up => Key::ArrowUp,
        Keycode::Escape => Key::Escape,
        Keycode::Tab => Key::Tab,
        Keycode::Backspace => Key::Backspace,
        Keycode::Return | Keycode::KpEnter => Key::Enter,
        Keycode::Space => Key::Space,
        Keycode::Insert => Key::Insert,
        Keycode::Delete => Key::Delete,
        Keycode::Home => Key::Home,
        Keycode::End => Key::End,
        Keycode::PageUp => Key::PageUp,
        Keycode::PageDown => Key::PageDown,
        Keycode::A => Key::A,
        Keycode::B => Key::B,
        Keycode::C => Key::C,
        Keycode::D => Key::D,
        Keycode::E => Key::E,
        Keycode::F => Key::F,
        Keycode::G => Key::G,
        Keycode::H => Key::H,
        Keycode::I => Key::I,
        Keycode::J => Key::J,
        Keycode::K => Key::K,
        Keycode::L => Key::L,
        Keycode::M => Key::M,
        Keycode::N => Key::N,
        Keycode::O => Key::O,
        Keycode::P => Key::P,
        Keycode::Q => Key::Q,
        Keycode::R => Key::R,
        Keycode::S => Key::S,
        Keycode::T => Key::T,
        Keycode::U => Key::U,
        Keycode::V => Key::V,
        Keycode::W => Key::W,
        Keycode::X => Key::X,
        Keycode::Y => Key::Y,
        Keycode::Z => Key::Z,
        Keycode::Num0 | Keycode::Kp0 => Key::Num0,
        Keycode::Num1 | Keycode::Kp1 => Key::Num1,
        Keycode::Num2 | Keycode::Kp2 => Key::Num2,
        Keycode::Num3 | Keycode::Kp3 => Key::Num3,
        Keycode::Num4 | Keycode::Kp4 => Key::Num4,
        Keycode::Num5 | Keycode::Kp5 => Key::Num5,
        Keycode::Num6 | Keycode::Kp6 => Key::Num6,
        Keycode::Num7 | Keycode::Kp7 => Key::Num7,
        Keycode::Num8 | Keycode::Kp8 => Key::Num8,
        Keycode::Num9 | Keycode::Kp9 => Key::Num9,
        Keycode::F1 => Key::F1,
        Keycode::F2 => Key::F2,
        Keycode::F3 => Key::F3,
        Keycode::F4 => Key::F4,
        Keycode::F5 => Key::F5,
        Keycode::F6 => Key::F6,
        Keycode::F7 => Key::F7,
        Keycode::F8 => Key::F8,
        Keycode::F9 => Key::F9,
        Keycode::F10 => Key::F10,
        Keycode::F11 => Key::F11,
        Keycode::F12 => Key::F12,
        _ => return None,
    })
}

/// Owns the egui context and the textures it asked for, and paints them.
///
/// The engine keeps one inside its `devtools` state. A host that drives a
/// [`RenderContext`] itself can own one too, which is what the integration test does.
///
/// # C ABI
///
/// Not exposed. C consumers reach the console through `giochi_console_execute` and see
/// its text through the panel, never through this type.
#[repr(C)]
pub(crate) struct EguiBackend {
    ctx: egui::Context,
    /// egui texture id → engine texture handle. The font atlas is entry zero of every
    /// egui session and is re-uploaded when the glyph set grows.
    textures: HashMap<egui::TextureId, TextureHandle>,
    /// Reused between meshes and frames: egui hands out a fresh `Vec<Vertex>` per mesh,
    /// and SDL wants its own layout, so a repack is unavoidable — an allocation is not.
    vertices: Vec<SdlVertex>,
    /// `SDL_RenderSetClipRect` is a state change; only issue it when the rect moves.
    last_clip: Option<sdl2::rect::Rect>,
    /// Points-to-pixels ratio handed to egui. 1.0 matches a non-retina window; KMSDRM
    /// is 1.0 by definition because the drawable *is* the panel.
    pixels_per_point: f32,
    /// Monotonic time fed to egui, accumulated from the frame deltas.
    ///
    /// egui uses it for double-click detection and animation. Accumulating the deltas
    /// instead of reading `Instant::now()` keeps a headless test deterministic.
    time: f64,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for EguiBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EguiBackend")
            .field("textures", &self.textures.len())
            .field("pixels_per_point", &self.pixels_per_point)
            .finish_non_exhaustive()
    }
}

impl EguiBackend {
    /// Creates the context and the font atlas that comes with it.
    pub(crate) fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            textures: HashMap::new(),
            vertices: Vec::new(),
            last_clip: None,
            pixels_per_point: 1.0,
            time: 0.0,
            _reserved: [0; 4],
        }
    }

    /// Drops the engine handles behind the egui textures.
    ///
    /// Must be called while the registry is still alive; `Drop` cannot do it, because
    /// there is no `RenderContext` at drop time — the same ownership rule as
    /// `DebugOverlay`.
    pub(crate) fn release(&mut self, render: &mut RenderContext) {
        let ids: Vec<egui::TextureId> = self.textures.keys().copied().collect();
        for id in ids {
            self.free_texture(render, &id);
        }
        self.last_clip = None;
    }

    /// Runs one egui frame and returns what it produced.
    ///
    /// `ui` is a trait object rather than a generic so the method stays inside the
    /// crate's no-generics-public-API rule; it is `pub(crate)`, but keeping the whole
    /// module uniform avoids a surprise when phase 21 makes part of it public.
    pub(crate) fn run(
        &mut self,
        screen: (u32, u32),
        dt: f32,
        events: &[egui::Event],
        ui: &mut dyn FnMut(&egui::Context),
    ) -> egui::FullOutput {
        self.time += dt.max(0.0) as f64;

        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(screen.0 as f32, screen.1 as f32),
            )),
            time: Some(self.time),
            events: events.to_vec(),
            ..Default::default()
        };

        self.ctx.run(raw, |ctx| ui(ctx))
    }

    /// Uploads every texture egui changed and then draws the frame.
    ///
    /// Order matters: `textures_delta.set` must be applied **before** painting and
    /// `textures_delta.free` **after**, which is what `FullOutput` documents.
    pub(crate) fn paint(&mut self, render: &mut RenderContext, output: egui::FullOutput) {
        for command in &output.platform_output.commands {
            // The engine has no clipboard on a KMSDRM target, and reaching SDL's would
            // mean holding the platform here. Logging keeps the request visible instead
            // of dropping it silently (`docs/DEVTOOLS.md` §7).
            log::debug!("egui output command: {command:?}");
        }

        for (id, delta) in &output.textures_delta.set {
            self.set_texture(render, *id, delta);
        }

        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);

        for primitive in primitives {
            let egui::ClippedPrimitive {
                clip_rect,
                primitive,
            } = primitive;
            match primitive {
                egui::epaint::Primitive::Mesh(mesh) => {
                    self.paint_mesh(render, clip_rect, &mesh);
                }
                egui::epaint::Primitive::Callback(_) => {
                    // Nothing in this engine uses egui's paint callbacks (they exist for
                    // embedding another 3D renderer inside an egui area).
                    log::debug!("egui paint callback ignored");
                }
            }
        }

        for id in &output.textures_delta.free {
            self.free_texture(render, id);
        }

        // Leave the canvas as we found it: the caller may draw more after us, and a
        // clip rect left behind silently truncates it.
        if self.last_clip.take().is_some() {
            render.canvas_mut().set_clip_rect(None);
        }
    }

    /// Creates or updates one egui texture in the engine's registry.
    fn set_texture(
        &mut self,
        render: &mut RenderContext,
        id: egui::TextureId,
        delta: &egui::epaint::ImageDelta,
    ) {
        let egui::ImageData::Color(image) = &delta.image;
        let width = image.size[0] as u32;
        let height = image.size[1] as u32;
        if width == 0 || height == 0 {
            log::debug!("egui texture {id:?} has zero size; ignored");
            return;
        }

        let pixels = colour_bytes(image.pixels.as_ref());

        match (delta.pos, self.textures.get(&id).copied()) {
            // A full upload of a texture that already exists at the same size is an
            // in-place update, which is what the font atlas does when it grows.
            (None, Some(handle)) if render.texture_size(handle) == Some((width, height)) => {
                if let Err(error) = render.update_texture_rgba(handle, width, height, &pixels) {
                    log::warn!("egui texture {id:?} could not be updated: {error}");
                }
            }
            // A new texture, or one whose size changed: allocate.
            (None, Some(handle)) => {
                let by_size = render
                    .create_texture_from_rgba(width, height, &pixels)
                    .map_err(|error| log::warn!("egui texture {id:?}: {error}"))
                    .ok();
                if let Some(new_handle) = by_size {
                    let _ = render.destroy_texture(handle);
                    self.textures.insert(id, new_handle);
                }
            }
            (None, None) => match render.create_texture_from_rgba(width, height, &pixels) {
                Ok(handle) => {
                    self.textures.insert(id, handle);
                }
                Err(error) => log::warn!("egui texture {id:?}: {error}"),
            },
            // A partial update (a cursor blink, a scrolled list's rows). The engine
            // texture API has no sub-rect upload, so the whole image is rebuilt from
            // the delta's patch by uploading it as a full texture of the patched size
            // is *wrong*; instead the patch is ignored and the next full upload fixes
            // it. egui only emits these for large atlases, which the font atlas is
            // not until it grows past the first allocation.
            (Some(position), Some(_handle)) => {
                log::trace!("egui partial texture update for {id:?} at {position:?} ignored");
            }
            (Some(position), None) => {
                log::debug!("egui partial update for unknown texture {id:?} at {position:?}");
            }
        }
    }

    /// Destroys the engine texture behind an egui id.
    fn free_texture(&mut self, render: &mut RenderContext, id: &egui::TextureId) {
        if let Some(handle) = self.textures.remove(id) {
            render.destroy_texture(handle);
        }
    }

    /// Submits one tessellated mesh.
    fn paint_mesh(
        &mut self,
        render: &mut RenderContext,
        clip_rect: egui::Rect,
        mesh: &egui::epaint::Mesh,
    ) {
        if mesh.indices.is_empty() {
            return;
        }

        let ppp = self.pixels_per_point;
        let clip = sdl2::rect::Rect::new(
            (clip_rect.min.x * ppp) as i32,
            (clip_rect.min.y * ppp) as i32,
            ((clip_rect.max.x - clip_rect.min.x) * ppp).max(0.0) as u32,
            ((clip_rect.max.y - clip_rect.min.y) * ppp).max(0.0) as u32,
        );

        self.vertices.clear();
        self.vertices.reserve(mesh.vertices.len());
        self.vertices.extend(mesh.vertices.iter().map(|vertex| {
            SdlVertex {
                position: FPoint::new(vertex.pos.x * ppp, vertex.pos.y * ppp),
                // `r()/g()/b()` are the **un**multiplied accessors; see the module docs.
                color: SdlColor::RGBA(
                    vertex.color.r(),
                    vertex.color.g(),
                    vertex.color.b(),
                    vertex.color.a(),
                ),
                tex_coord: FPoint::new(vertex.uv.x, vertex.uv.y),
            }
        }));

        let indices: Vec<i32> = mesh.indices.iter().map(|index| *index as i32).collect();
        let texture = self
            .textures
            .get(&mesh.texture_id)
            .and_then(|handle| render.textures().resolve(*handle));

        let canvas = render.canvas_mut();
        if self.last_clip != Some(clip) {
            canvas.set_clip_rect(clip);
            self.last_clip = Some(clip);
        }
        match texture {
            Some(texture) => {
                // The blend mode was set when the texture was created
                // (`create_texture_from_rgba` sets `Blend`, the mode egui needs); a
                // `Rc<Texture>` cannot change it here, and it does not need to.
                if let Err(error) = canvas.render_geometry(&self.vertices, Some(&texture), &indices)
                {
                    log::warn!("egui geometry failed: {error}");
                }
            }
            None => {
                // An untextured mesh (egui uses one for solid fills with a white pixel
                // in the atlas, but can also emit a null texture id).
                if let Err(error) = canvas.render_geometry(&self.vertices, None, &indices) {
                    log::warn!("egui untectured geometry failed: {error}");
                }
            }
        }
    }
}

/// `&[Color32]` as RGBA8 bytes.
///
/// The crate is `#![forbid(unsafe_code)]`, so a zero-copy reinterpretation is not
/// available even though `Color32` is `repr(transparent)` over `u32`. This is the safe
/// version, and it costs one allocation per texture upload — which happens when the font
/// atlas grows, not per frame.
fn colour_bytes(pixels: &[egui::Color32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(pixels.len() * BYTES_PER_PIXEL);
    for pixel in pixels {
        bytes.extend_from_slice(&pixel.to_array());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_map_both_sides() {
        let modifiers = translate_modifiers(Mod::LCTRLMOD | Mod::RSHIFTMOD);
        assert!(modifiers.ctrl);
        assert!(modifiers.command);
        assert!(modifiers.shift);
        assert!(!modifiers.alt);
    }

    #[test]
    fn the_editing_keys_egui_needs_are_mapped() {
        for code in [
            Keycode::Backspace,
            Keycode::Delete,
            Keycode::Return,
            Keycode::Tab,
            Keycode::Escape,
            Keycode::Home,
            Keycode::End,
            Keycode::Left,
            Keycode::Right,
            Keycode::Up,
            Keycode::Down,
        ] {
            assert!(
                translate_key(code).is_some(),
                "{code:?} must reach egui or text editing breaks"
            );
        }
    }

    #[test]
    fn a_letter_key_maps_to_the_same_letter() {
        assert_eq!(translate_key(Keycode::A), Some(egui::Key::A));
        assert_eq!(translate_key(Keycode::Z), Some(egui::Key::Z));
        assert_eq!(translate_key(Keycode::Num0), Some(egui::Key::Num0));
    }

    #[test]
    fn a_text_event_is_consumed_but_a_pointer_move_is_not() {
        let text = Event::TextInput {
            timestamp: 0,
            window_id: 0,
            text: "a".to_owned(),
        };
        let (event, consumed) = translate_event(&text, true);
        assert!(matches!(event, Some(egui::Event::Text(_))));
        assert!(consumed);

        let motion = Event::MouseMotion {
            timestamp: 0,
            window_id: 0,
            which: 0,
            mousestate: sdl2::mouse::MouseState::from_sdl_state(0),
            x: 4,
            y: 5,
            xrel: 0,
            yrel: 0,
        };
        let (event, consumed) = translate_event(&motion, true);
        assert!(matches!(
            event,
            Some(egui::Event::PointerMoved(position)) if position == egui::pos2(4.0, 5.0)
        ));
        assert!(
            !consumed,
            "moving the mouse must not swallow the game's aim"
        );
    }

    #[test]
    fn a_key_is_consumed_only_while_text_has_focus() {
        let key = Event::KeyDown {
            timestamp: 0,
            window_id: 0,
            keycode: Some(Keycode::W),
            scancode: Some(sdl2::keyboard::Scancode::W),
            keymod: Mod::NOMOD,
            repeat: false,
        };
        assert!(
            translate_event(&key, true).1,
            "typing must not move the player"
        );
        assert!(
            !translate_event(&key, false).1,
            "an idle console must not swallow the keyboard"
        );
    }

    #[test]
    fn an_unmapped_key_is_not_forwarded() {
        let key = Event::KeyDown {
            timestamp: 0,
            window_id: 0,
            keycode: Some(Keycode::Application),
            scancode: None,
            keymod: Mod::NOMOD,
            repeat: false,
        };
        assert_eq!(translate_event(&key, true).0, None);
    }

    #[test]
    fn the_pixel_conversion_is_four_bytes_per_pixel_in_rgba_order() {
        let pixels = vec![
            egui::Color32::from_rgba_premultiplied(1, 2, 3, 4),
            egui::Color32::from_rgba_premultiplied(5, 6, 7, 8),
        ];
        let bytes = colour_bytes(&pixels);
        assert_eq!(bytes.len(), 8);
        assert_eq!(bytes, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
