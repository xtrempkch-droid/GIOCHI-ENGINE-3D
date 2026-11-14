//! Billboards de partículas 3D desenhados pelo caminho instanciado quando disponível.
//!
//! # Examples
//!
//! ```no_run
//! use giochi_engine_3d::particles::BillboardParticleSystem;
//! use giochi_engine_3d::math3d::Vec3;
//!
//! let mut particles = BillboardParticleSystem::new(1_000, Vec3::new(0.0, -9.8, 0.0), 7)?;
//! particles.emit_burst(Vec3::ZERO, 64, 4.0, 1.0, 0.2)?;
//! particles.update(1.0 / 60.0)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod billboard;

pub use billboard::BillboardParticleSystem;
