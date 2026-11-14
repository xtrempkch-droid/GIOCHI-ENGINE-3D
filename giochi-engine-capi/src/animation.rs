//! Animação de spritesheet — Part VII da ABI C.
//!
//! Mesmo desenho da parte VI: `SpriteAnimation` tem um `Vec` de frames, portanto o C
//! recebe um handle. Uma animação criada com `giochi_animator_create` cobre a faixa
//! horizontal de uma textura — o caso que um spritesheet simples tem — e
//! `giochi_animator_draw` desenha o frame corrente pela câmara do engine.
//!
//! `mode`: `0` toca uma vez e para, `1` repete, `2` vai e volta sem repetir os extremos.

use std::cell::RefCell;

use giochi_engine::animation::{PlaybackMode, SpriteAnimation};
use giochi_engine::{Rect, Vec2};

use crate::abi_types::{GIOCHI_ERROR_INVALID_ARGUMENT, GIOCHI_ERROR_INVALID_HANDLE};
use crate::state::{self, guard, set_last_error};

const RENDER_HINT: &str = "an animation can only be drawn inside a render callback \
     (or outside a run); updating belongs in update";

thread_local! {
    /// Slot `0` is handle `1`; destroyed animations leave `None`, so handles are never
    /// reused.
    static ANIMATIONS: RefCell<Vec<Option<SpriteAnimation>>> = const { RefCell::new(Vec::new()) };
}

fn index_of(handle: u64) -> Option<usize> {
    handle.checked_sub(1).map(|index| index as usize)
}

fn not_live(handle: u64) -> i32 {
    set_last_error(&format!("animation handle {handle} is not live"));
    GIOCHI_ERROR_INVALID_HANDLE
}

fn mode_from_c(mode: u32) -> Option<PlaybackMode> {
    match mode {
        0 => Some(PlaybackMode::Once),
        1 => Some(PlaybackMode::Loop),
        2 => Some(PlaybackMode::PingPong),
        _ => None,
    }
}

fn with_animation<R>(handle: u64, body: impl FnOnce(&mut SpriteAnimation) -> R) -> Result<R, i32> {
    ANIMATIONS.with(|cell| {
        let mut animations = cell.borrow_mut();
        match index_of(handle).and_then(|index| animations.get_mut(index)) {
            Some(Some(animation)) => Ok(body(animation)),
            _ => Err(not_live(handle)),
        }
    })
}

fn report(error: giochi_engine::animation::AnimationError) -> i32 {
    set_last_error(&error.to_string());
    GIOCHI_ERROR_INVALID_ARGUMENT
}

/// Creates a horizontal spritesheet animation and returns its handle.
///
/// `frame_width`/`frame_height` are in texture pixels; `frame_duration` is seconds per
/// frame. `mode` is `0` once, `1` loop, `2` ping-pong. Returns `0` on failure.
#[no_mangle]
pub extern "C" fn giochi_animator_create(
    texture: giochi_engine::TextureHandle,
    frame_width: f32,
    frame_height: f32,
    frame_count: u32,
    frame_duration: f32,
    mode: u32,
) -> u64 {
    guard(0, || {
        if texture == 0 {
            set_last_error("the texture handle is 0");
            return 0;
        }
        let Some(mode) = mode_from_c(mode) else {
            set_last_error("unknown playback mode");
            return 0;
        };
        match SpriteAnimation::from_strip(
            texture,
            Vec2::new(frame_width, frame_height),
            frame_count,
            frame_duration,
            mode,
        ) {
            Ok(animation) => ANIMATIONS.with(|cell| {
                let mut animations = cell.borrow_mut();
                animations.push(Some(animation));
                animations.len() as u64
            }),
            Err(error) => {
                report(error);
                0
            }
        }
    })
}

/// Destroys an animation.
#[no_mangle]
pub extern "C" fn giochi_animator_destroy(handle: u64) -> i32 {
    guard(GIOCHI_ERROR_INVALID_HANDLE, || {
        ANIMATIONS.with(|cell| {
            let mut animations = cell.borrow_mut();
            match index_of(handle).and_then(|index| animations.get_mut(index)) {
                Some(slot @ Some(_)) => {
                    *slot = None;
                    0
                }
                _ => not_live(handle),
            }
        })
    })
}

/// Advances the animation by `dt` seconds.
#[no_mangle]
pub extern "C" fn giochi_animator_update(handle: u64, dt: f32) -> i32 {
    guard(GIOCHI_ERROR_INVALID_HANDLE, || {
        match with_animation(handle, |animation| animation.update(dt).map_err(report)) {
            Ok(Ok(())) => 0,
            Ok(Err(code)) => code,
            Err(code) => code,
        }
    })
}

/// Which frame is showing right now (`-1` for an invalid handle).
#[no_mangle]
pub extern "C" fn giochi_animator_current_frame(handle: u64) -> i32 {
    guard(-1, || {
        ANIMATIONS.with(|cell| {
            let animations = cell.borrow();
            match index_of(handle).and_then(|index| animations.get(index)) {
                Some(Some(animation)) => {
                    i32::try_from(animation.current_frame_index()).unwrap_or(-1)
                }
                _ => -1,
            }
        })
    })
}

/// Restarts the animation from the first frame and leaves it playing.
#[no_mangle]
pub extern "C" fn giochi_animator_reset(handle: u64) -> i32 {
    guard(GIOCHI_ERROR_INVALID_HANDLE, || {
        match with_animation(handle, |animation| {
            animation.reset();
            animation.play();
        }) {
            Ok(()) => 0,
            Err(code) => code,
        }
    })
}

/// Pauses the animation on the current frame.
#[no_mangle]
pub extern "C" fn giochi_animator_pause(handle: u64) -> i32 {
    guard(GIOCHI_ERROR_INVALID_HANDLE, || {
        match with_animation(handle, |animation| animation.pause()) {
            Ok(()) => 0,
            Err(code) => code,
        }
    })
}

/// Resumes a paused animation.
#[no_mangle]
pub extern "C" fn giochi_animator_play(handle: u64) -> i32 {
    guard(GIOCHI_ERROR_INVALID_HANDLE, || {
        match with_animation(handle, |animation| animation.play()) {
            Ok(()) => 0,
            Err(code) => code,
        }
    })
}

/// Whether a one-shot animation reached its last frame (`1`) or not (`0`).
#[no_mangle]
pub extern "C" fn giochi_animator_finished(handle: u64) -> i32 {
    guard(0, || {
        ANIMATIONS.with(|cell| {
            let animations = cell.borrow();
            match index_of(handle).and_then(|index| animations.get(index)) {
                Some(Some(animation)) => i32::from(animation.is_finished()),
                _ => 0,
            }
        })
    })
}

/// Draws the current frame into `(x, y, width, height)` through the engine's camera.
///
/// Only valid inside a `render` callback (or outside a run).
#[no_mangle]
pub extern "C" fn giochi_animator_draw(
    handle: u64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> i32 {
    guard(GIOCHI_ERROR_INVALID_HANDLE, || {
        match state::with_render(RENDER_HINT, |render| {
            ANIMATIONS.with(|cell| {
                let animations = cell.borrow();
                match index_of(handle).and_then(|index| animations.get(index)) {
                    Some(Some(animation)) => {
                        match animation.draw(render, Rect::new(x, y, width, height)) {
                            Ok(()) => 0,
                            Err(error) => state::fail(&error),
                        }
                    }
                    _ => not_live(handle),
                }
            })
        }) {
            Ok(code) => code,
            Err(code) => code,
        }
    })
}

const _: () = assert!(GIOCHI_ERROR_INVALID_HANDLE < 0);
const _: () = assert!(GIOCHI_ERROR_INVALID_ARGUMENT < 0);
