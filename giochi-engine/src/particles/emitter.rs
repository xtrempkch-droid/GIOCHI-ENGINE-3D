//! Regiões de emissão 2D e gerador determinístico de números aleatórios.

use std::fmt::Debug;

use crate::math::{Rect, Vec2};

use super::system::ParticleError;

/// Gerador determinístico usado para amostrar emissores.
///
/// A sequência depende apenas da seed, o que torna testes e efeitos reprodutíveis.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::ParticleRng;
///
/// let mut random = ParticleRng::new(7);
/// let sample = random.next_f32();
/// assert!((0.0..1.0).contains(&sample));
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticleRng {
    state: u64,
    _reserved: [u32; 4],
}

impl ParticleRng {
    /// Cria o gerador com uma seed; zero é substituído por uma seed não nula.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::ParticleRng;
    ///
    /// let mut random = ParticleRng::new(42);
    /// assert_ne!(random.next_u64(), 0);
    /// ```
    pub const fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9e37_79b9_7f4a_7c15
            } else {
                seed
            },
            _reserved: [0; 4],
        }
    }

    /// Produz o próximo valor de 64 bits.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::ParticleRng;
    ///
    /// let mut random = ParticleRng::new(1);
    /// let first = random.next_u64();
    /// assert_ne!(first, random.next_u64());
    /// ```
    pub fn next_u64(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.state = value;
        value.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Produz um `f32` no intervalo semiaberto `0.0..1.0`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::ParticleRng;
    ///
    /// let mut random = ParticleRng::new(3);
    /// assert!((0.0..1.0).contains(&random.next_f32()));
    /// ```
    pub fn next_f32(&mut self) -> f32 {
        let mantissa = (self.next_u64() >> 40) as u32;
        mantissa as f32 / (1u32 << 24) as f32
    }

    pub(crate) fn signed_unit(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }
}

/// Amostrador da posição inicial de partículas.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::{ParticleEmitter, ParticleRng, PointEmitter};
/// use giochi_engine::Vec2;
///
/// let emitter = PointEmitter::new(Vec2::new(4.0, 8.0))?;
/// let position = emitter.sample(&mut ParticleRng::new(1));
/// assert_eq!(position, Vec2::new(4.0, 8.0));
/// # Ok::<(), giochi_engine::particles::ParticleError>(())
/// ```
pub trait ParticleEmitter: Debug {
    /// Devolve uma posição amostrada usando `random`.
    fn sample(&self, random: &mut ParticleRng) -> Vec2;
}

/// Emite partículas num ponto.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::{ParticleEmitter, ParticleRng, PointEmitter};
/// use giochi_engine::Vec2;
///
/// let emitter = PointEmitter::new(Vec2::new(12.0, 24.0))?;
/// assert_eq!(emitter.sample(&mut ParticleRng::new(2)), Vec2::new(12.0, 24.0));
/// # Ok::<(), giochi_engine::particles::ParticleError>(())
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointEmitter {
    position: Vec2,
    _reserved: [u32; 4],
}

impl PointEmitter {
    /// Cria um emissor num ponto finito.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::PointEmitter;
    /// use giochi_engine::Vec2;
    ///
    /// let emitter = PointEmitter::new(Vec2::new(10.0, 20.0))?;
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn new(position: Vec2) -> Result<Self, ParticleError> {
        if !position.is_finite() {
            return Err(ParticleError::InvalidEmitter(
                "a posição do emissor precisa ser finita",
            ));
        }
        Ok(Self {
            position,
            _reserved: [0; 4],
        })
    }
}

impl ParticleEmitter for PointEmitter {
    fn sample(&self, _random: &mut ParticleRng) -> Vec2 {
        self.position
    }
}

/// Emite partículas ao longo de um segmento.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::{LineEmitter, ParticleEmitter, ParticleRng};
/// use giochi_engine::Vec2;
///
/// let emitter = LineEmitter::new(Vec2::ZERO, Vec2::new(10.0, 0.0))?;
/// let point = emitter.sample(&mut ParticleRng::new(4));
/// assert!((0.0..=10.0).contains(&point.x));
/// # Ok::<(), giochi_engine::particles::ParticleError>(())
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineEmitter {
    start: Vec2,
    end: Vec2,
    _reserved: [u32; 4],
}

impl LineEmitter {
    /// Cria um emissor entre dois pontos finitos.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::LineEmitter;
    /// use giochi_engine::Vec2;
    ///
    /// let emitter = LineEmitter::new(Vec2::ZERO, Vec2::new(20.0, 5.0))?;
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn new(start: Vec2, end: Vec2) -> Result<Self, ParticleError> {
        if !start.is_finite() || !end.is_finite() {
            return Err(ParticleError::InvalidEmitter(
                "os extremos do emissor linear precisam ser finitos",
            ));
        }
        Ok(Self {
            start,
            end,
            _reserved: [0; 4],
        })
    }
}

impl ParticleEmitter for LineEmitter {
    fn sample(&self, random: &mut ParticleRng) -> Vec2 {
        self.start + (self.end - self.start) * random.next_f32()
    }
}

/// Emite partículas uniformemente dentro de um círculo.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::{CircleEmitter, ParticleEmitter, ParticleRng};
/// use giochi_engine::Vec2;
///
/// let emitter = CircleEmitter::new(Vec2::ZERO, 5.0)?;
/// assert!(emitter.sample(&mut ParticleRng::new(5)).length() <= 5.0);
/// # Ok::<(), giochi_engine::particles::ParticleError>(())
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CircleEmitter {
    center: Vec2,
    radius: f32,
    _reserved: [u32; 4],
}

impl CircleEmitter {
    /// Cria um emissor circular com raio finito e não negativo.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::CircleEmitter;
    /// use giochi_engine::Vec2;
    ///
    /// let emitter = CircleEmitter::new(Vec2::new(40.0, 30.0), 12.0)?;
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn new(center: Vec2, radius: f32) -> Result<Self, ParticleError> {
        if !center.is_finite() || !radius.is_finite() || radius < 0.0 {
            return Err(ParticleError::InvalidEmitter(
                "centro e raio do emissor circular precisam ser válidos",
            ));
        }
        Ok(Self {
            center,
            radius,
            _reserved: [0; 4],
        })
    }
}

impl ParticleEmitter for CircleEmitter {
    fn sample(&self, random: &mut ParticleRng) -> Vec2 {
        let angle = random.next_f32() * std::f32::consts::TAU;
        let radius = self.radius * random.next_f32().sqrt();
        self.center + Vec2::new(angle.cos() * radius, angle.sin() * radius)
    }
}

/// Emite partículas uniformemente dentro de um retângulo.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::{AreaEmitter, ParticleEmitter, ParticleRng};
/// use giochi_engine::{Rect, Vec2};
///
/// let emitter = AreaEmitter::new(Rect::new(1.0, 2.0, 10.0, 20.0))?;
/// assert!(emitter.area().contains(emitter.sample(&mut ParticleRng::new(6))));
/// # Ok::<(), giochi_engine::particles::ParticleError>(())
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaEmitter {
    area: Rect,
    _reserved: [u32; 4],
}

impl AreaEmitter {
    /// Cria um emissor a partir de um retângulo finito com área positiva.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::AreaEmitter;
    /// use giochi_engine::Rect;
    ///
    /// let emitter = AreaEmitter::new(Rect::new(0.0, 0.0, 16.0, 8.0))?;
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub fn new(area: Rect) -> Result<Self, ParticleError> {
        let finite = [area.x, area.y, area.w, area.h]
            .iter()
            .all(|value| value.is_finite());
        if !finite || area.is_empty() {
            return Err(ParticleError::InvalidEmitter(
                "a área do emissor precisa ser finita e positiva",
            ));
        }
        Ok(Self {
            area,
            _reserved: [0; 4],
        })
    }

    /// Devolve a área usada pelo emissor.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::AreaEmitter;
    /// use giochi_engine::Rect;
    ///
    /// let emitter = AreaEmitter::new(Rect::new(1.0, 2.0, 3.0, 4.0))?;
    /// assert_eq!(emitter.area().w, 3.0);
    /// # Ok::<(), giochi_engine::particles::ParticleError>(())
    /// ```
    pub const fn area(&self) -> Rect {
        self.area
    }
}

impl ParticleEmitter for AreaEmitter {
    fn sample(&self, random: &mut ParticleRng) -> Vec2 {
        Vec2::new(
            self.area.x + random.next_f32() * self.area.w,
            self.area.y + random.next_f32() * self.area.h,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_random_generators_produce_the_same_sequence() {
        let mut left = ParticleRng::new(123);
        let mut right = ParticleRng::new(123);

        for _ in 0..16 {
            assert_eq!(left.next_u64(), right.next_u64());
        }
    }

    #[test]
    fn emitters_sample_only_inside_their_shape() {
        let mut random = ParticleRng::new(81);
        let line = LineEmitter::new(Vec2::new(1.0, 3.0), Vec2::new(7.0, 9.0)).expect("valid line");
        let circle = CircleEmitter::new(Vec2::ZERO, 4.0).expect("valid circle");
        let area = AreaEmitter::new(Rect::new(10.0, 20.0, 5.0, 6.0)).expect("valid area");

        for _ in 0..100 {
            let point = line.sample(&mut random);
            assert!((1.0..=7.0).contains(&point.x));
            assert!((3.0..=9.0).contains(&point.y));
            assert!(circle.sample(&mut random).length() <= 4.0);
            assert!(area.area().contains(area.sample(&mut random)));
        }
    }

    #[test]
    fn emitters_reject_non_finite_or_degenerate_inputs() {
        assert!(PointEmitter::new(Vec2::new(f32::NAN, 0.0)).is_err());
        assert!(LineEmitter::new(Vec2::ZERO, Vec2::new(f32::INFINITY, 0.0)).is_err());
        assert!(CircleEmitter::new(Vec2::ZERO, -1.0).is_err());
        assert!(AreaEmitter::new(Rect::ZERO).is_err());
    }
}
