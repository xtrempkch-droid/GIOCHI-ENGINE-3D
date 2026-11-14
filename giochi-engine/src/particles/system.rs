//! Pool de partículas 2D e integração com o batch do renderer.

use thiserror::Error;

use crate::error::GiochiError;
use crate::math::{Color, Vec2};
use crate::render::{RenderContext, Sprite, TextureHandle};

use super::curve::{Curve, CurveValue};
use super::emitter::{ParticleEmitter, ParticleRng};

/// Capacidade inicial padrão do pool.
pub const DEFAULT_PARTICLE_CAPACITY: usize = 5_000;
/// Limite superior para impedir reservas acidentais descontroladas.
pub const MAX_PARTICLE_CAPACITY: usize = 100_000;

/// Configuração copiada para cada partícula emitida.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::{ParticleConfig, ParticleSystem, PointEmitter};
/// use giochi_engine::Vec2;
///
/// let config = ParticleConfig {
///     gravity: Vec2::new(0.0, 120.0),
///     ..ParticleConfig::default()
/// };
/// let mut particles = ParticleSystem::new(1, config)?;
/// particles.emit(&PointEmitter::new(Vec2::ZERO)?, 10)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleConfig {
    /// Velocidade inicial, em unidades de mundo por segundo.
    pub velocity: Vec2,
    /// Variação aleatória simétrica aplicada a cada componente da velocidade.
    pub velocity_spread: Vec2,
    /// Aceleração constante aplicada durante a vida da partícula.
    pub gravity: Vec2,
    /// Arrasto exponencial por segundo; deve ser não negativo.
    pub drag: f32,
    /// Vida em segundos; deve ser positiva.
    pub lifetime: f32,
    /// Tamanho inicial em unidades de mundo.
    pub start_size: f32,
    /// Tamanho final em unidades de mundo.
    pub end_size: f32,
    /// Cor no início da vida.
    pub start_color: Color,
    /// Cor no fim da vida.
    pub end_color: Color,
    /// Reservado para extensão ABI compatível; deve ser zero.
    pub _reserved: [u32; 4],
}

impl Default for ParticleConfig {
    fn default() -> Self {
        Self {
            velocity: Vec2::ZERO,
            velocity_spread: Vec2::ZERO,
            gravity: Vec2::ZERO,
            drag: 0.0,
            lifetime: 1.0,
            start_size: 8.0,
            end_size: 0.0,
            start_color: Color::WHITE,
            end_color: Color::TRANSPARENT,
            _reserved: [0; 4],
        }
    }
}

impl ParticleConfig {
    fn validate(self) -> Result<Self, ParticleError> {
        if !self.velocity.is_finite()
            || !self.velocity_spread.is_finite()
            || !self.gravity.is_finite()
            || self.velocity_spread.x < 0.0
            || self.velocity_spread.y < 0.0
            || !self.drag.is_finite()
            || self.drag < 0.0
            || !self.lifetime.is_finite()
            || self.lifetime <= 0.0
            || !self.start_size.is_finite()
            || self.start_size < 0.0
            || !self.end_size.is_finite()
            || self.end_size < 0.0
        {
            return Err(ParticleError::InvalidConfig(
                "valores numéricos inválidos na configuração de partículas",
            ));
        }
        Ok(self)
    }
}

/// Erros de configuração e atualização do sistema de partículas.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::{ParticleConfig, ParticleError, ParticleSystem};
///
/// let error = ParticleSystem::with_capacity(1, 0, ParticleConfig::default());
/// assert!(matches!(error, Err(ParticleError::InvalidCapacity)));
/// ```
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ParticleError {
    /// A capacidade é zero ou excede [`MAX_PARTICLE_CAPACITY`].
    #[error("capacidade do pool fora do intervalo permitido")]
    InvalidCapacity,
    /// Um valor numérico da configuração não é válido.
    #[error("{0}")]
    InvalidConfig(&'static str),
    /// Os parâmetros do emissor são inválidos.
    #[error("{0}")]
    InvalidEmitter(&'static str),
    /// O passo de tempo é negativo ou não finito.
    #[error("delta precisa ser finito e não negativo")]
    InvalidDelta,
    /// Um emissor personalizado produziu posição inválida.
    #[error("o emissor produziu uma posição ou velocidade não finita")]
    InvalidSpawn,
}

#[derive(Debug, Clone, Copy)]
struct Particle {
    position: Vec2,
    velocity: Vec2,
    gravity: Vec2,
    drag: f32,
    age: f32,
    lifetime: f32,
    start_size: f32,
    end_size: f32,
    start_color: Color,
    end_color: Color,
}

/// Sistema 2D CPU com pool limitado e desenho integrado ao batch de sprites.
///
/// O sistema mantém uma única textura por pool, preserva a ordem de emissão e não
/// aumenta o seu `Vec` durante a simulação. A capacidade padrão é
/// [`DEFAULT_PARTICLE_CAPACITY`].
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::{ParticleConfig, ParticleSystem, PointEmitter};
/// use giochi_engine::Vec2;
///
/// let mut smoke = ParticleSystem::new(1, ParticleConfig::default())?;
/// let emitter = PointEmitter::new(Vec2::new(80.0, 60.0))?;
/// let emitted = smoke.emit(&emitter, 20)?;
/// assert_eq!(emitted, 20);
/// smoke.update(1.0 / 60.0)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[repr(C)]
#[derive(Debug)]
pub struct ParticleSystem {
    texture: TextureHandle,
    capacity: usize,
    config: ParticleConfig,
    particles: Vec<Particle>,
    progress_curve: Curve<f32>,
    random: ParticleRng,
    _reserved: [u32; 4],
}

impl ParticleSystem {
    /// Cria um pool com a capacidade padrão.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{ParticleConfig, ParticleSystem};
    ///
    /// let particles = ParticleSystem::new(3, ParticleConfig::default())?;
    /// assert_eq!(particles.capacity(), 5_000);
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn new(texture: TextureHandle, config: ParticleConfig) -> Result<Self, ParticleError> {
        Self::with_capacity(texture, DEFAULT_PARTICLE_CAPACITY, config)
    }

    /// Cria um pool com capacidade fixa e uma textura.
    ///
    /// A capacidade tem de estar entre 1 e [`MAX_PARTICLE_CAPACITY`]. A emissão que
    /// ultrapassa o espaço disponível é limitada ao espaço livre.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{ParticleConfig, ParticleSystem};
    ///
    /// let particles = ParticleSystem::with_capacity(3, 128, ParticleConfig::default())?;
    /// assert_eq!(particles.capacity(), 128);
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn with_capacity(
        texture: TextureHandle,
        capacity: usize,
        config: ParticleConfig,
    ) -> Result<Self, ParticleError> {
        if capacity == 0 || capacity > MAX_PARTICLE_CAPACITY {
            return Err(ParticleError::InvalidCapacity);
        }
        Ok(Self {
            texture,
            capacity,
            config: config.validate()?,
            particles: Vec::with_capacity(capacity),
            progress_curve: Curve::linear(0.0, 1.0),
            random: ParticleRng::new(1),
            _reserved: [0; 4],
        })
    }

    /// Replaces the random seed for repeatable effects.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{ParticleConfig, ParticleSystem};
    ///
    /// let particles = ParticleSystem::new(2, ParticleConfig::default())?
    ///     .with_seed(42);
    /// assert_eq!(particles.capacity(), 5_000);
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.random = ParticleRng::new(seed);
        self
    }

    /// Returns the texture shared by all particles in this pool.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{ParticleConfig, ParticleSystem};
    ///
    /// let particles = ParticleSystem::new(9, ParticleConfig::default())?;
    /// assert_eq!(particles.texture(), 9);
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn texture(&self) -> TextureHandle {
        self.texture
    }

    /// Returns the reserved pool capacity.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{ParticleConfig, ParticleSystem};
    ///
    /// let particles = ParticleSystem::with_capacity(1, 64, ParticleConfig::default())?;
    /// assert_eq!(particles.capacity(), 64);
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the number of active particles.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{ParticleConfig, ParticleSystem};
    ///
    /// let particles = ParticleSystem::new(1, ParticleConfig::default())?;
    /// assert_eq!(particles.len(), 0);
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn len(&self) -> usize {
        self.particles.len()
    }

    /// Indicates whether the pool has no active particles.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{ParticleConfig, ParticleSystem};
    ///
    /// let particles = ParticleSystem::new(1, ParticleConfig::default())?;
    /// assert!(particles.is_empty());
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
    }

    /// Returns a copy of the configuration used by future emissions.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{ParticleConfig, ParticleSystem};
    ///
    /// let particles = ParticleSystem::new(1, ParticleConfig::default())?;
    /// assert_eq!(particles.config().lifetime, 1.0);
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn config(&self) -> ParticleConfig {
        self.config
    }

    /// Replaces the configuration used by future emissions.
    ///
    /// Existing particles retain the values copied when they were emitted.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{ParticleConfig, ParticleSystem};
    ///
    /// let mut particles = ParticleSystem::new(1, ParticleConfig::default())?;
    /// particles.set_config(ParticleConfig { lifetime: 2.0, ..ParticleConfig::default() })?;
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn set_config(&mut self, config: ParticleConfig) -> Result<(), ParticleError> {
        self.config = config.validate()?;
        Ok(())
    }

    /// Replaces the curve that maps normalized age to interpolation progress.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{Curve, ParticleConfig, ParticleSystem};
    ///
    /// let mut particles = ParticleSystem::new(1, ParticleConfig::default())?;
    /// particles.set_progress_curve(Curve::ease_in(0.0, 1.0));
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn set_progress_curve(&mut self, curve: Curve<f32>) {
        self.progress_curve = curve;
    }

    /// Emits up to `count` particles and returns the number actually added.
    ///
    /// If a custom emitter returns a non-finite position, the call fails and restores
    /// the pool to its original length.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{ParticleConfig, ParticleSystem, PointEmitter};
    /// use giochi_engine::Vec2;
    ///
    /// let mut particles = ParticleSystem::with_capacity(1, 4, ParticleConfig::default())?;
    /// let emitter = PointEmitter::new(Vec2::ZERO)?;
    /// assert_eq!(particles.emit(&emitter, 10)?, 4);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn emit(
        &mut self,
        emitter: &dyn ParticleEmitter,
        count: usize,
    ) -> Result<usize, ParticleError> {
        let original_len = self.particles.len();
        let available = self.capacity.saturating_sub(original_len);
        let count = count.min(available);
        for _ in 0..count {
            let position = emitter.sample(&mut self.random);
            let velocity = Vec2::new(
                self.config.velocity.x + self.random.signed_unit() * self.config.velocity_spread.x,
                self.config.velocity.y + self.random.signed_unit() * self.config.velocity_spread.y,
            );
            if !position.is_finite() || !velocity.is_finite() {
                self.particles.truncate(original_len);
                return Err(ParticleError::InvalidSpawn);
            }
            self.particles.push(Particle {
                position,
                velocity,
                gravity: self.config.gravity,
                drag: self.config.drag,
                age: 0.0,
                lifetime: self.config.lifetime,
                start_size: self.config.start_size,
                end_size: self.config.end_size,
                start_color: self.config.start_color,
                end_color: self.config.end_color,
            });
        }
        Ok(count)
    }

    /// Advances particles by a finite, non-negative number of seconds.
    ///
    /// Expired particles are removed without changing the pool's capacity.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{ParticleConfig, ParticleSystem};
    ///
    /// let mut particles = ParticleSystem::new(1, ParticleConfig::default())?;
    /// particles.update(1.0 / 60.0)?;
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn update(&mut self, delta: f32) -> Result<(), ParticleError> {
        if !delta.is_finite() || delta < 0.0 {
            return Err(ParticleError::InvalidDelta);
        }
        if delta == 0.0 || self.particles.is_empty() {
            return Ok(());
        }

        self.particles.retain_mut(|particle| {
            particle.age += delta;
            if particle.age >= particle.lifetime {
                return false;
            }

            particle.velocity =
                (particle.velocity + particle.gravity * delta) * (-particle.drag * delta).exp();
            particle.position = particle.position + particle.velocity * delta;
            particle.position.is_finite() && particle.velocity.is_finite()
        });
        Ok(())
    }

    /// Removes active particles while retaining the allocated pool memory.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{ParticleConfig, ParticleSystem, PointEmitter};
    /// use giochi_engine::Vec2;
    ///
    /// let mut particles = ParticleSystem::new(1, ParticleConfig::default())?;
    /// particles.emit(&PointEmitter::new(Vec2::ZERO)?, 4)?;
    /// particles.clear();
    /// assert!(particles.is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn clear(&mut self) {
        self.particles.clear();
    }

    /// Queues all active particles in the renderer's order-preserving sprite batch.
    ///
    /// Returns the number of particles queued. The caller controls when the shared
    /// batch is flushed, so other scene sprites can be submitted before or after.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{ParticleConfig, ParticleSystem};
    /// use giochi_engine::RenderContext;
    ///
    /// # fn render(system: &giochi_engine::particles::ParticleSystem, renderer: &mut RenderContext) -> Result<(), giochi_engine::GiochiError> {
    /// let queued = system.render(renderer)?;
    /// println!("queued {queued} particles");
    /// # Ok(())
    /// # }
    /// ```
    pub fn render(&self, renderer: &mut RenderContext) -> Result<usize, GiochiError> {
        let mut rendered = 0;
        for particle in &self.particles {
            let normalized_age = (particle.age / particle.lifetime).clamp(0.0, 1.0);
            let progress = self.progress_curve.sample(normalized_age).clamp(0.0, 1.0);
            let size = particle.start_size + (particle.end_size - particle.start_size) * progress;
            if size <= 0.0 {
                continue;
            }

            let tint = particle
                .start_color
                .interpolate(particle.end_color, progress);
            let sprite = Sprite::new(
                self.texture,
                particle.position.x - size * 0.5,
                particle.position.y - size * 0.5,
                size,
                size,
            );
            renderer.draw_sprite_tinted(&sprite, tint)?;
            rendered += 1;
        }
        Ok(rendered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::particles::{PointEmitter, MAX_PARTICLE_CAPACITY};

    #[test]
    fn pool_is_fixed_and_emission_is_capped() {
        let mut particles =
            ParticleSystem::with_capacity(1, 8, ParticleConfig::default()).expect("valid pool");
        let emitter = PointEmitter::new(Vec2::ZERO).expect("valid point");
        let allocation = particles.particles.as_ptr();

        assert_eq!(particles.emit(&emitter, 10).expect("emission"), 8);
        assert_eq!(particles.particles.as_ptr(), allocation);
        assert_eq!(particles.len(), 8);
        assert_eq!(particles.capacity(), 8);
    }

    #[test]
    fn update_applies_gravity_drag_and_expiration() {
        let config = ParticleConfig {
            velocity: Vec2::new(10.0, 0.0),
            gravity: Vec2::new(0.0, 10.0),
            drag: 1.0,
            lifetime: 1.0,
            ..ParticleConfig::default()
        };
        let mut particles =
            ParticleSystem::with_capacity(1, 2, config).expect("valid configuration");
        let emitter = PointEmitter::new(Vec2::ZERO).expect("valid point");
        particles.emit(&emitter, 1).expect("emission");

        particles.update(0.5).expect("valid delta");
        let particle = particles.particles[0];
        assert!(particle.position.x > 0.0);
        assert!(particle.position.y > 0.0);
        assert!(particle.velocity.x < 10.0);

        particles.update(0.5).expect("valid delta");
        assert!(particles.is_empty());
        assert_eq!(particles.particles.capacity(), 2);
    }

    #[test]
    fn invalid_config_delta_and_capacity_are_reported() {
        assert!(matches!(
            ParticleSystem::with_capacity(1, 0, ParticleConfig::default()),
            Err(ParticleError::InvalidCapacity)
        ));
        assert!(matches!(
            ParticleSystem::with_capacity(1, MAX_PARTICLE_CAPACITY + 1, ParticleConfig::default()),
            Err(ParticleError::InvalidCapacity)
        ));
        assert!(matches!(
            ParticleSystem::new(
                1,
                ParticleConfig {
                    lifetime: f32::NAN,
                    ..ParticleConfig::default()
                }
            ),
            Err(ParticleError::InvalidConfig(_))
        ));
        let mut particles =
            ParticleSystem::new(1, ParticleConfig::default()).expect("valid configuration");
        assert_eq!(
            particles.update(f32::INFINITY),
            Err(ParticleError::InvalidDelta)
        );
    }

    #[test]
    fn invalid_custom_emitter_rolls_back_partial_emission() {
        use std::cell::Cell;

        #[derive(Debug)]
        struct InvalidEmitter(Cell<usize>);

        impl ParticleEmitter for InvalidEmitter {
            fn sample(&self, random: &mut ParticleRng) -> Vec2 {
                let emitted = self.0.get();
                self.0.set(emitted + 1);
                if emitted == 0 {
                    Vec2::ZERO
                } else {
                    let _ = random.next_f32();
                    Vec2::new(f32::NAN, 0.0)
                }
            }
        }

        let mut particles = ParticleSystem::with_capacity(1, 16, ParticleConfig::default())
            .expect("valid configuration")
            .with_seed(2);
        let result = particles.emit(&InvalidEmitter(Cell::new(0)), 16);

        assert_eq!(result, Err(ParticleError::InvalidSpawn));
        assert!(particles.is_empty());
    }
}
