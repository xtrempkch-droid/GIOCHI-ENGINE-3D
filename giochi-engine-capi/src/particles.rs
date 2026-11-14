//! Partículas — Part VI da ABI C.
//!
//! Um `ParticleSystem` tem um `Vec` de partículas por dentro, logo não atravessa a
//! fronteira como valor: o C recebe um handle `uint64_t` e o sistema vive num registo
//! thread-local, como as texturas e os sons.
//!
//! # A convenção de sempre
//!
//! `0` nunca é um handle válido; `int32_t` devolve `0` em sucesso e um código negativo
//! em erro, com a explicação em `giochi_get_last_error()`. Desenhar só funciona dentro
//! do callback de `render` — `giochi_particles_draw` fora dele devolve
//! `GIOCHI_ERROR_SCENE`, como qualquer outro desenho.

use std::cell::RefCell;

use giochi_engine::math::Color;
use giochi_engine::particles::{
    ParticleConfig, ParticleSystem, PointEmitter, DEFAULT_PARTICLE_CAPACITY,
};
use giochi_engine::Vec2;

use crate::abi_types::{GIOCHI_ERROR_INVALID_ARGUMENT, GIOCHI_ERROR_INVALID_HANDLE};
use crate::state::{self, guard, set_last_error};

const RENDER_HINT: &str =
    "particles can only be drawn inside a render callback (or outside a run); \
     updating belongs in update";

thread_local! {
    /// Slot `0` is handle `1`. A destroyed system leaves `None` in place, so handles
    /// are never reused — a stale handle is an error, not a different emitter.
    static SYSTEMS: RefCell<Vec<Option<ParticleSystem>>> = const { RefCell::new(Vec::new()) };
}

/// The description of an emitter's behaviour, as the C side fills it in.
///
/// Mirrors `ParticleConfig` field for field. Kept as its own type rather than exporting
/// the Rust struct so the header stays independent of the engine's internals.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GiochiParticleConfig {
    /// Initial velocity, in world units per second.
    pub velocity_x: f32,
    /// Initial velocity, in world units per second.
    pub velocity_y: f32,
    /// Symmetric random spread applied to each velocity component.
    pub velocity_spread_x: f32,
    /// Symmetric random spread applied to each velocity component.
    pub velocity_spread_y: f32,
    /// Constant acceleration during the particle's life.
    pub gravity_x: f32,
    /// Constant acceleration during the particle's life.
    pub gravity_y: f32,
    /// Exponential drag per second; must not be negative.
    pub drag: f32,
    /// Lifetime in seconds; must be positive.
    pub lifetime: f32,
    /// Initial size, in world units.
    pub start_size: f32,
    /// Final size, in world units.
    pub end_size: f32,
    /// Colour at the start of the life.
    pub start_color: Color,
    /// Colour at the end of the life.
    pub end_color: Color,
    /// Reserved for ABI-compatible extension; must be zero.
    pub _reserved: [u32; 4],
}

impl Default for GiochiParticleConfig {
    fn default() -> Self {
        Self::from_config(ParticleConfig::default())
    }
}

impl GiochiParticleConfig {
    fn from_config(config: ParticleConfig) -> Self {
        GiochiParticleConfig {
            velocity_x: config.velocity.x,
            velocity_y: config.velocity.y,
            velocity_spread_x: config.velocity_spread.x,
            velocity_spread_y: config.velocity_spread.y,
            gravity_x: config.gravity.x,
            gravity_y: config.gravity.y,
            drag: config.drag,
            lifetime: config.lifetime,
            start_size: config.start_size,
            end_size: config.end_size,
            start_color: config.start_color,
            end_color: config.end_color,
            _reserved: [0; 4],
        }
    }

    fn to_config(self) -> ParticleConfig {
        ParticleConfig {
            velocity: Vec2::new(self.velocity_x, self.velocity_y),
            velocity_spread: Vec2::new(self.velocity_spread_x, self.velocity_spread_y),
            gravity: Vec2::new(self.gravity_x, self.gravity_y),
            drag: self.drag,
            lifetime: self.lifetime,
            start_size: self.start_size,
            end_size: self.end_size,
            start_color: self.start_color,
            end_color: self.end_color,
            ..ParticleConfig::default()
        }
    }
}

fn index_of(handle: u64) -> Option<usize> {
    handle.checked_sub(1).map(|index| index as usize)
}

fn not_live(handle: u64) -> i32 {
    set_last_error(&format!("particle handle {handle} is not live"));
    GIOCHI_ERROR_INVALID_HANDLE
}

/// Runs `body` with the live system behind `handle`.
fn with_system<R>(handle: u64, body: impl FnOnce(&mut ParticleSystem) -> R) -> Result<R, i32> {
    SYSTEMS.with(|cell| {
        let mut systems = cell.borrow_mut();
        match index_of(handle).and_then(|index| systems.get_mut(index)) {
            Some(Some(system)) => Ok(body(system)),
            _ => Err(not_live(handle)),
        }
    })
}

fn report(error: giochi_engine::ParticleError) -> i32 {
    set_last_error(&error.to_string());
    GIOCHI_ERROR_INVALID_ARGUMENT
}

/// Creates a particle system over `texture` and returns its handle.
///
/// `capacity` is the pool size (`0` uses the default of 5 000). `config` is copied;
/// a `NULL` config uses the default emitter. Returns `0` on failure.
#[no_mangle]
pub extern "C" fn giochi_particles_create(
    texture: giochi_engine::TextureHandle,
    capacity: u32,
    config: *const GiochiParticleConfig,
) -> u64 {
    guard(0, || {
        if texture == 0 {
            set_last_error("the texture handle is 0");
            return 0;
        }
        let config = if config.is_null() {
            ParticleConfig::default()
        } else {
            // SAFETY: non-NULL, and the contract says it points at a config. It is read
            // here and never stored.
            unsafe { *config }.to_config()
        };
        let capacity = if capacity == 0 {
            DEFAULT_PARTICLE_CAPACITY
        } else {
            capacity as usize
        };
        match ParticleSystem::with_capacity(texture, capacity, config) {
            Ok(system) => SYSTEMS.with(|cell| {
                let mut systems = cell.borrow_mut();
                systems.push(Some(system));
                systems.len() as u64
            }),
            Err(error) => {
                report(error);
                0
            }
        }
    })
}

/// Destroys a particle system.
///
/// Returns `GIOCHI_ERROR_INVALID_HANDLE` for `0` or an already-destroyed handle.
#[no_mangle]
pub extern "C" fn giochi_particles_destroy(handle: u64) -> i32 {
    guard(GIOCHI_ERROR_INVALID_HANDLE, || {
        SYSTEMS.with(|cell| {
            let mut systems = cell.borrow_mut();
            match index_of(handle).and_then(|index| systems.get_mut(index)) {
                Some(slot @ Some(_)) => {
                    *slot = None;
                    0
                }
                _ => not_live(handle),
            }
        })
    })
}

/// Emits `count` particles at `(x, y)`.
///
/// Returns how many were actually emitted — the pool caps at its capacity — or a
/// negative error code. Emitting into a full pool returns `0` and is not an error.
#[no_mangle]
pub extern "C" fn giochi_particles_emit(handle: u64, x: f32, y: f32, count: u32) -> i32 {
    guard(GIOCHI_ERROR_INVALID_HANDLE, || {
        match with_system(handle, |system| {
            // The emitter is built first so an invalid position is reported before the
            // pool is touched.
            let emitter = PointEmitter::new(Vec2::new(x, y)).map_err(|error| {
                let _ = error;
                set_last_error("the emission position is not finite");
                GIOCHI_ERROR_INVALID_ARGUMENT
            })?;
            system
                .emit(&emitter, count as usize)
                .map_err(report)
                .map(|emitted| i32::try_from(emitted).unwrap_or(i32::MAX))
        }) {
            Ok(result) => result.unwrap_or_else(|code| code),
            Err(code) => code,
        }
    })
}

/// Advances the live particles by `dt` seconds.
#[no_mangle]
pub extern "C" fn giochi_particles_update(handle: u64, dt: f32) -> i32 {
    guard(GIOCHI_ERROR_INVALID_HANDLE, || {
        match with_system(handle, |system| system.update(dt).map_err(report)) {
            Ok(Ok(())) => 0,
            Ok(Err(code)) => code,
            Err(code) => code,
        }
    })
}

/// Draws the live particles through the engine's camera.
///
/// Only valid inside a `render` callback (or outside a run).
#[no_mangle]
pub extern "C" fn giochi_particles_draw(handle: u64) -> i32 {
    guard(GIOCHI_ERROR_INVALID_HANDLE, || {
        match state::with_render(RENDER_HINT, |render| {
            SYSTEMS.with(|cell| {
                let systems = cell.borrow();
                match index_of(handle).and_then(|index| systems.get(index)) {
                    Some(Some(system)) => match system.render(render) {
                        Ok(_) => 0,
                        Err(error) => state::fail(&error),
                    },
                    _ => not_live(handle),
                }
            })
        }) {
            Ok(code) => code,
            Err(code) => code,
        }
    })
}

/// How many particles are alive right now (`0` for an invalid handle).
#[no_mangle]
pub extern "C" fn giochi_particles_alive(handle: u64) -> u32 {
    guard(0, || {
        SYSTEMS.with(|cell| {
            let systems = cell.borrow();
            match index_of(handle).and_then(|index| systems.get(index)) {
                Some(Some(system)) => system.len() as u32,
                _ => 0,
            }
        })
    })
}

/// Copies the default emitter configuration into `out_config`.
#[no_mangle]
pub extern "C" fn giochi_particles_default_config(out_config: *mut GiochiParticleConfig) -> i32 {
    guard(GIOCHI_ERROR_INVALID_ARGUMENT, || {
        if out_config.is_null() {
            set_last_error("out_config is NULL");
            return GIOCHI_ERROR_INVALID_ARGUMENT;
        }
        // SAFETY: checked for NULL above; the contract says it is writable.
        unsafe { *out_config = GiochiParticleConfig::default() };
        0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_is_96_bytes_and_matches_the_header() {
        // The header declares this struct and a consumer compiled against it reads it by
        // offset. The Python smoke test used to declare `start_color` as a `uint32_t`, which
        // made its buffer 64 bytes where the library writes 96: the write overran it and
        // every field after `end_size` came from the wrong place. Pinning the size here is
        // the cheapest guard against that class of mismatch.
        assert_eq!(core::mem::size_of::<GiochiParticleConfig>(), 96);
        assert_eq!(core::mem::align_of::<GiochiParticleConfig>(), 4);
        assert_eq!(core::mem::size_of::<Color>(), 20);
        // `_reserved` must be the last field or the offsets above are meaningless.
        let config = GiochiParticleConfig::default();
        assert_eq!(config._reserved, [0; 4]);
    }

    #[test]
    fn creating_requires_a_texture_and_a_live_handle_to_use() {
        assert_eq!(giochi_particles_create(0, 16, core::ptr::null()), 0);
        assert!(state::last_error_text().contains("texture"));

        let handle = 9_999u64;
        assert_eq!(
            giochi_particles_destroy(handle),
            GIOCHI_ERROR_INVALID_HANDLE
        );
        assert_eq!(giochi_particles_alive(handle), 0);
        assert_eq!(
            giochi_particles_emit(handle, 0.0, 0.0, 1),
            GIOCHI_ERROR_INVALID_HANDLE
        );
        assert_eq!(
            giochi_particles_update(handle, 0.016),
            GIOCHI_ERROR_INVALID_HANDLE
        );
        assert_eq!(
            giochi_particles_default_config(core::ptr::null_mut()),
            GIOCHI_ERROR_INVALID_ARGUMENT
        );
    }
}
