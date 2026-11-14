//! Sistema CPU 3D com partículas orientadas para a câmara.

use giochi_engine::particles::{CurveValue, ParticleError, ParticleRng, MAX_PARTICLE_CAPACITY};

use crate::math3d::{Camera3D, Color3D, Mat4, Quat, Transform3D, Vec3};
use crate::{MeshHandle, RenderContext3D, Result3D, ShaderHandle};

#[derive(Debug, Clone, Copy)]
struct Particle {
    position: Vec3,
    velocity: Vec3,
    age: f32,
    lifetime: f32,
    size: f32,
}

/// Partículas 3D CPU desenhadas como billboards virados para a câmara.
///
/// O pool e o buffer de matrizes reservam a capacidade no construtor. O render ordena
/// partículas de trás para a frente e usa `draw_instanced`; em drivers sem suporte, o
/// renderer conserva a imagem correta usando o caminho de fallback.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine_3d::math3d::Vec3;
/// use giochi_engine_3d::particles::BillboardParticleSystem;
///
/// let mut particles = BillboardParticleSystem::new(2_000, Vec3::new(0.0, -9.8, 0.0), 9)?;
/// particles.emit_burst(Vec3::ZERO, 100, 8.0, 1.5, 0.25)?;
/// particles.update(1.0 / 60.0)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[repr(C)]
#[derive(Debug)]
pub struct BillboardParticleSystem {
    particles: Vec<Particle>,
    instances: Vec<Mat4>,
    capacity: usize,
    gravity: Vec3,
    random: ParticleRng,
    _reserved: [u32; 4],
}

impl BillboardParticleSystem {
    /// Cria um pool fixo, com gravidade e seed determinística.
    ///
    /// A capacidade tem de estar entre 1 e `MAX_PARTICLE_CAPACITY`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine_3d::math3d::Vec3;
    /// use giochi_engine_3d::particles::BillboardParticleSystem;
    ///
    /// let particles = BillboardParticleSystem::new(512, Vec3::new(0.0, -9.8, 0.0), 1)?;
    /// assert_eq!(particles.capacity(), 512);
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn new(capacity: usize, gravity: Vec3, seed: u64) -> Result<Self, ParticleError> {
        if capacity == 0 || capacity > MAX_PARTICLE_CAPACITY {
            return Err(ParticleError::InvalidCapacity);
        }
        if !gravity.is_finite() {
            return Err(ParticleError::InvalidConfig(
                "a gravidade 3D precisa ser finita",
            ));
        }

        Ok(Self {
            particles: Vec::with_capacity(capacity),
            instances: Vec::with_capacity(capacity),
            capacity,
            gravity,
            random: ParticleRng::new(seed),
            _reserved: [0; 4],
        })
    }

    /// Devolve a capacidade fixa do pool.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine_3d::math3d::Vec3;
    /// use giochi_engine_3d::particles::BillboardParticleSystem;
    ///
    /// let particles = BillboardParticleSystem::new(128, Vec3::ZERO, 1)?;
    /// assert_eq!(particles.capacity(), 128);
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Devolve a contagem de partículas ativas.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine_3d::math3d::Vec3;
    /// use giochi_engine_3d::particles::BillboardParticleSystem;
    ///
    /// let particles = BillboardParticleSystem::new(128, Vec3::ZERO, 1)?;
    /// assert_eq!(particles.len(), 0);
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn len(&self) -> usize {
        self.particles.len()
    }

    /// Indica se o pool não tem partículas ativas.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine_3d::math3d::Vec3;
    /// use giochi_engine_3d::particles::BillboardParticleSystem;
    ///
    /// let particles = BillboardParticleSystem::new(128, Vec3::ZERO, 1)?;
    /// assert!(particles.is_empty());
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
    }

    /// Adiciona uma partícula, ou devolve `false` quando o pool está cheio.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine_3d::math3d::Vec3;
    /// use giochi_engine_3d::particles::BillboardParticleSystem;
    ///
    /// let mut particles = BillboardParticleSystem::new(16, Vec3::ZERO, 1)?;
    /// assert!(particles.emit(Vec3::ZERO, Vec3::UP, 1.0, 0.25)?);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn emit(
        &mut self,
        position: Vec3,
        velocity: Vec3,
        lifetime: f32,
        size: f32,
    ) -> Result<bool, ParticleError> {
        if !position.is_finite()
            || !velocity.is_finite()
            || !lifetime.is_finite()
            || lifetime <= 0.0
            || !size.is_finite()
            || size <= 0.0
        {
            return Err(ParticleError::InvalidConfig(
                "posição, velocidade, vida e tamanho 3D precisam ser válidos",
            ));
        }
        if self.particles.len() == self.capacity {
            return Ok(false);
        }
        self.particles.push(Particle {
            position,
            velocity,
            age: 0.0,
            lifetime,
            size,
        });
        Ok(true)
    }

    /// Emite uma explosão com direções uniformes na esfera.
    ///
    /// Devolve o número de partículas adicionadas; a emissão é limitada pelo espaço
    /// livre no pool.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine_3d::math3d::Vec3;
    /// use giochi_engine_3d::particles::BillboardParticleSystem;
    ///
    /// let mut particles = BillboardParticleSystem::new(512, Vec3::ZERO, 2)?;
    /// let emitted = particles.emit_burst(Vec3::ZERO, 200, 6.0, 1.0, 0.15)?;
    /// assert_eq!(emitted, 200);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn emit_burst(
        &mut self,
        origin: Vec3,
        count: usize,
        speed: f32,
        lifetime: f32,
        size: f32,
    ) -> Result<usize, ParticleError> {
        if !origin.is_finite()
            || !speed.is_finite()
            || speed < 0.0
            || !lifetime.is_finite()
            || lifetime <= 0.0
            || !size.is_finite()
            || size <= 0.0
        {
            return Err(ParticleError::InvalidConfig(
                "parâmetros inválidos para a explosão 3D",
            ));
        }

        let count = count.min(self.capacity.saturating_sub(self.particles.len()));
        for _ in 0..count {
            let z = self.random.next_f32() * 2.0 - 1.0;
            let angle = self.random.next_f32() * std::f32::consts::TAU;
            let radial = (1.0 - z * z).sqrt();
            let direction = Vec3::new(radial * angle.cos(), radial * angle.sin(), z);
            self.particles.push(Particle {
                position: origin,
                velocity: Vec3::from_glam(direction.to_glam() * speed),
                age: 0.0,
                lifetime,
                size,
            });
        }
        Ok(count)
    }

    /// Avança a simulação por um delta finito e não negativo.
    ///
    /// Partículas expiradas são removidas sem alterar a capacidade reservada.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine_3d::math3d::Vec3;
    /// use giochi_engine_3d::particles::BillboardParticleSystem;
    ///
    /// let mut particles = BillboardParticleSystem::new(128, Vec3::ZERO, 1)?;
    /// particles.update(1.0 / 60.0)?;
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn update(&mut self, delta: f32) -> Result<(), ParticleError> {
        if !delta.is_finite() || delta < 0.0 {
            return Err(ParticleError::InvalidDelta);
        }
        if delta == 0.0 {
            return Ok(());
        }

        let gravity = self.gravity;
        self.particles.retain_mut(|particle| {
            particle.age += delta;
            if particle.age >= particle.lifetime {
                return false;
            }
            particle.velocity =
                Vec3::from_glam(particle.velocity.to_glam() + gravity.to_glam() * delta);
            particle.position =
                Vec3::from_glam(particle.position.to_glam() + particle.velocity.to_glam() * delta);
            particle.position.is_finite() && particle.velocity.is_finite()
        });
        Ok(())
    }

    /// Removes active particles while keeping both reserved buffers.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine_3d::math3d::Vec3;
    /// use giochi_engine_3d::particles::BillboardParticleSystem;
    ///
    /// let mut particles = BillboardParticleSystem::new(128, Vec3::ZERO, 1)?;
    /// particles.emit(Vec3::ZERO, Vec3::UP, 1.0, 0.25)?;
    /// particles.clear();
    /// assert!(particles.is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn clear(&mut self) {
        self.particles.clear();
        self.instances.clear();
    }

    /// Ordena e desenha billboards numa chamada instanciada quando o driver suporta.
    ///
    /// `mesh` deve ser um quad centrado no plano XY, com a normal local em +Z. Ative
    /// `GlState::blend` antes de `begin_frame` para transparência alfa.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine_3d::math3d::{Camera3D, Color3D};
    /// use giochi_engine_3d::particles::BillboardParticleSystem;
    /// use giochi_engine_3d::{RenderContext3D, MeshHandle, ShaderHandle};
    ///
    /// # fn draw(system: &mut BillboardParticleSystem, renderer: &mut RenderContext3D, quad: MeshHandle, shader: ShaderHandle, camera: &Camera3D) -> Result<(), giochi_engine_3d::Error3D> {
    /// system.render(renderer, quad, shader, camera, Color3D::WHITE)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn render(
        &mut self,
        renderer: &mut RenderContext3D,
        quad: MeshHandle,
        shader: ShaderHandle,
        camera: &Camera3D,
        color: Color3D,
    ) -> Result3D<u32> {
        self.sort_back_to_front(camera.position);

        self.instances.clear();
        for particle in &self.particles {
            let progress = (particle.age / particle.lifetime).clamp(0.0, 1.0);
            let size = particle.size * (1.0 - progress);
            if size <= 0.0 {
                continue;
            }
            let to_camera = camera.position.to_glam() - particle.position.to_glam();
            let rotation = if to_camera.length_squared() > 0.0 {
                Quat::from_glam(glam::Quat::from_rotation_arc(
                    glam::Vec3::Z,
                    to_camera.normalize(),
                ))
            } else {
                Quat::IDENTITY
            };
            let transform =
                Transform3D::new(particle.position, rotation, Vec3::new(size, size, 1.0));
            self.instances.push(transform.matrix());
        }

        renderer.draw_instanced(quad, shader, &self.instances, camera, color)
    }

    fn sort_back_to_front(&mut self, camera_position: Vec3) {
        let camera_position = camera_position.to_glam();
        self.particles.sort_unstable_by(|left, right| {
            let left_distance = left.position.to_glam().distance_squared(camera_position);
            let right_distance = right.position.to_glam().distance_squared(camera_position);
            right_distance.total_cmp(&left_distance)
        });
    }
}

impl CurveValue for Vec3 {
    fn interpolate(self, other: Self, amount: f32) -> Self {
        Vec3::from_glam(self.to_glam().lerp(other.to_glam(), amount))
    }

    fn is_finite(self) -> bool {
        self.is_finite()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_is_fixed_and_bursts_are_capped() {
        let mut particles = BillboardParticleSystem::new(8, Vec3::ZERO, 11).expect("valid pool");
        let allocation = particles.particles.as_ptr();

        assert_eq!(
            particles
                .emit_burst(Vec3::ZERO, 12, 4.0, 1.0, 0.25)
                .expect("valid burst"),
            8
        );
        assert_eq!(particles.particles.as_ptr(), allocation);
        assert_eq!(particles.len(), 8);
        assert_eq!(particles.instances.capacity(), 8);
    }

    #[test]
    fn update_applies_gravity_and_removes_expired_particles() {
        let mut particles =
            BillboardParticleSystem::new(2, Vec3::new(0.0, -10.0, 0.0), 3).expect("valid pool");
        particles
            .emit(Vec3::ZERO, Vec3::ZERO, 0.5, 1.0)
            .expect("valid particle");

        particles.update(0.25).expect("valid delta");
        assert!(particles.particles[0].position.y < 0.0);
        particles.update(0.25).expect("valid delta");
        assert!(particles.is_empty());
        assert_eq!(particles.particles.capacity(), 2);
    }

    #[test]
    fn particle_positions_are_sorted_back_to_front_for_alpha() {
        let mut particles = BillboardParticleSystem::new(3, Vec3::ZERO, 4).expect("valid pool");
        particles
            .emit(Vec3::new(0.0, 0.0, -2.0), Vec3::ZERO, 1.0, 1.0)
            .expect("valid particle");
        particles
            .emit(Vec3::new(0.0, 0.0, -8.0), Vec3::ZERO, 1.0, 1.0)
            .expect("valid particle");
        particles
            .emit(Vec3::new(0.0, 0.0, -4.0), Vec3::ZERO, 1.0, 1.0)
            .expect("valid particle");

        particles.sort_back_to_front(Vec3::ZERO);

        assert_eq!(
            particles
                .particles
                .iter()
                .map(|particle| particle.position.z)
                .collect::<Vec<_>>(),
            vec![-8.0, -4.0, -2.0]
        );
    }

    #[test]
    fn core_curves_accept_the_3d_vector_type() {
        let curve = giochi_engine::particles::Curve::linear(Vec3::ZERO, Vec3::new(2.0, 4.0, 6.0));

        assert_eq!(curve.sample(0.5), Vec3::new(1.0, 2.0, 3.0));
    }
}
