//! Sistema de partículas CPU, sem trabalho automático quando não é usado.
//!
//! Uma cena é dona dos seus sistemas e chama `update` durante a simulação e `render`
//! durante o desenho. O pool reserva a capacidade no construtor; emitir e atualizar não
//! aumentam essa capacidade.
//!
//! # Examples
//!
//! ```no_run
//! use giochi_engine::particles::{ParticleConfig, ParticleSystem, PointEmitter};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut particles = ParticleSystem::new(1, ParticleConfig::default())?;
//! let emitter = PointEmitter::new(giochi_engine::Vec2::new(100.0, 100.0))?;
//! particles.emit(&emitter, 32)?;
//! particles.update(1.0 / 60.0)?;
//! # Ok(())
//! # }
//! ```

pub mod curve;
pub mod emitter;
pub mod presets;
pub mod system;

pub use curve::{Curve, CurveError, CurveValue, Interpolation, Keyframe};
pub use emitter::{
    AreaEmitter, CircleEmitter, LineEmitter, ParticleEmitter, ParticleRng, PointEmitter,
};
pub use system::{
    ParticleConfig, ParticleError, ParticleSystem, DEFAULT_PARTICLE_CAPACITY, MAX_PARTICLE_CAPACITY,
};
