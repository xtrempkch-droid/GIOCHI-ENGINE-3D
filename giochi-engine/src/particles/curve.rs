//! Curvas keyframe para valores usados por sistemas de partículas.

use thiserror::Error;

use crate::math::{Color, Vec2};

/// Interpolação usada entre um keyframe e o seguinte.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::{Curve, Interpolation};
///
/// let fade = Curve::ease_in(0.0, 1.0);
/// assert!(fade.sample(0.5) < 0.5);
/// let _linear = Curve::linear(0.0, 1.0);
/// let _mode = Interpolation::Linear;
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// Avança a uma velocidade constante.
    Linear,
    /// Começa devagar e acelera (`t²`).
    EaseIn,
}

/// Valor interpolável por uma [`Curve`].
///
/// A implementação padrão cobre `f32`, [`Color`] e [`Vec2`]. Crates que possuem
/// outros tipos podem implementar esta trait para usá-los em curvas.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::CurveValue;
///
/// fn midpoint<T: CurveValue>(from: T, to: T) -> T {
///     from.interpolate(to, 0.5)
/// }
/// ```
pub trait CurveValue: Copy {
    /// Interpola entre `self` e `other`, com `amount` no intervalo `0..=1`.
    fn interpolate(self, other: Self, amount: f32) -> Self;

    /// Indica se o valor pode ser usado com segurança numa curva.
    fn is_finite(self) -> bool;
}

impl CurveValue for f32 {
    fn interpolate(self, other: Self, amount: f32) -> Self {
        self + (other - self) * amount
    }

    fn is_finite(self) -> bool {
        self.is_finite()
    }
}

impl CurveValue for Vec2 {
    fn interpolate(self, other: Self, amount: f32) -> Self {
        self + (other - self) * amount
    }

    fn is_finite(self) -> bool {
        self.is_finite()
    }
}

impl CurveValue for Color {
    fn interpolate(self, other: Self, amount: f32) -> Self {
        let channel = |start: u8, end: u8| {
            (f32::from(start) + (f32::from(end) - f32::from(start)) * amount)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Color::new(
            channel(self.r, other.r),
            channel(self.g, other.g),
            channel(self.b, other.b),
            channel(self.a, other.a),
        )
    }

    fn is_finite(self) -> bool {
        true
    }
}

/// Um ponto de uma curva e a interpolação aplicada até ao próximo ponto.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::{Interpolation, Keyframe};
///
/// let point = Keyframe::new(0.5, 12.0, Interpolation::EaseIn);
/// assert_eq!(point.time, 0.5);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keyframe<T> {
    /// Posição temporal do ponto, em ordem crescente.
    pub time: f32,
    /// Valor guardado no ponto.
    pub value: T,
    /// Regra usada até ao próximo ponto.
    pub interpolation: Interpolation,
    _reserved: [u32; 4],
}

impl<T> Keyframe<T> {
    /// Cria um keyframe.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{Interpolation, Keyframe};
    ///
    /// let start = Keyframe::new(0.0, 0.0, Interpolation::Linear);
    /// assert_eq!(start.value, 0.0);
    /// ```
    pub const fn new(time: f32, value: T, interpolation: Interpolation) -> Self {
        Self {
            time,
            value,
            interpolation,
            _reserved: [0; 4],
        }
    }
}

/// Erros ao construir uma curva inválida.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::{Curve, CurveError, Keyframe, Interpolation};
///
/// let error = Curve::new(vec![Keyframe::new(
///     0.0,
///     f32::NAN,
///     Interpolation::Linear,
/// )]);
/// assert_eq!(error, Err(CurveError::NonFiniteValue));
/// ```
#[repr(C)]
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum CurveError {
    /// A curva não contém pontos.
    #[error("a curva precisa de pelo menos um keyframe")]
    Empty,
    /// Um instante ou valor não é finito.
    #[error("os instantes e valores da curva precisam ser finitos")]
    NonFiniteValue,
    /// Os instantes não estão em ordem estritamente crescente.
    #[error("os instantes da curva precisam ser estritamente crescentes")]
    NonIncreasingTime,
}

/// Curva de valores interpolados por keyframes.
///
/// Os keyframes são mantidos na ordem fornecida e amostrados com pesquisa binária.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::particles::{Curve, Interpolation, Keyframe};
///
/// let curve = Curve::new(vec![
///     Keyframe::new(0.0, 0.0, Interpolation::Linear),
///     Keyframe::new(1.0, 10.0, Interpolation::Linear),
/// ])?;
/// assert_eq!(curve.sample(0.5), 5.0);
/// # Ok::<(), giochi_engine::particles::CurveError>(())
/// ```
#[repr(C)]
#[derive(Debug, Clone, PartialEq)]
pub struct Curve<T> {
    keyframes: Vec<Keyframe<T>>,
    _reserved: [u32; 4],
}

impl<T: CurveValue> Curve<T> {
    /// Cria uma curva validando tempos crescentes e valores finitos.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::{Curve, Interpolation, Keyframe};
    ///
    /// let curve = Curve::new(vec![
    ///     Keyframe::new(0.0, 1.0, Interpolation::Linear),
    ///     Keyframe::new(1.0, 2.0, Interpolation::Linear),
    /// ])?;
    /// assert_eq!(curve.sample(1.0), 2.0);
    /// # Ok::<(), giochi_engine::particles::CurveError>(())
    /// ```
    pub fn new(keyframes: Vec<Keyframe<T>>) -> Result<Self, CurveError> {
        if keyframes.is_empty() {
            return Err(CurveError::Empty);
        }

        let mut previous_time = None;
        for keyframe in &keyframes {
            if !keyframe.time.is_finite() || !keyframe.value.is_finite() {
                return Err(CurveError::NonFiniteValue);
            }
            if previous_time.is_some_and(|time| keyframe.time <= time) {
                return Err(CurveError::NonIncreasingTime);
            }
            previous_time = Some(keyframe.time);
        }

        Ok(Self {
            keyframes,
            _reserved: [0; 4],
        })
    }

    /// Cria uma curva linear entre dois valores nos tempos 0 e 1.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::Curve;
    ///
    /// let curve = Curve::linear(2.0, 6.0);
    /// assert_eq!(curve.sample(0.5), 4.0);
    /// ```
    pub fn linear(start: T, end: T) -> Self {
        Self {
            keyframes: vec![
                Keyframe::new(0.0, start, Interpolation::Linear),
                Keyframe::new(1.0, end, Interpolation::Linear),
            ],
            _reserved: [0; 4],
        }
    }

    /// Cria uma curva `ease-in` entre dois valores nos tempos 0 e 1.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::Curve;
    ///
    /// let curve = Curve::ease_in(0.0, 1.0);
    /// assert_eq!(curve.sample(0.5), 0.25);
    /// ```
    pub fn ease_in(start: T, end: T) -> Self {
        Self {
            keyframes: vec![
                Keyframe::new(0.0, start, Interpolation::EaseIn),
                Keyframe::new(1.0, end, Interpolation::Linear),
            ],
            _reserved: [0; 4],
        }
    }

    /// Devolve os keyframes imutáveis da curva.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::Curve;
    ///
    /// let curve = Curve::linear(0.0, 1.0);
    /// assert_eq!(curve.keyframes().len(), 2);
    /// ```
    pub fn keyframes(&self) -> &[Keyframe<T>] {
        &self.keyframes
    }

    /// Amostra a curva; tempos fora do intervalo são limitados aos extremos.
    ///
    /// Um tempo não finito devolve o primeiro valor, evitando que `NaN` se propague
    /// para a geometria.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::particles::Curve;
    ///
    /// let curve = Curve::linear(0.0, 1.0);
    /// assert_eq!(curve.sample(2.0), 1.0);
    /// ```
    pub fn sample(&self, time: f32) -> T {
        let first = &self.keyframes[0];
        let last = &self.keyframes[self.keyframes.len() - 1];
        if !time.is_finite() || time <= first.time {
            return first.value;
        }
        if time >= last.time {
            return last.value;
        }

        let right = self
            .keyframes
            .partition_point(|keyframe| keyframe.time < time);
        let left = &self.keyframes[right - 1];
        let next = &self.keyframes[right];
        let amount = (time - left.time) / (next.time - left.time);
        let amount = match left.interpolation {
            Interpolation::Linear => amount,
            Interpolation::EaseIn => amount * amount,
        };
        left.value.interpolate(next.value, amount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_linear_and_ease_in_segments() {
        let linear = Curve::linear(0.0, 10.0);
        let ease = Curve::ease_in(0.0, 1.0);

        assert_eq!(linear.sample(0.5), 5.0);
        assert_eq!(ease.sample(0.5), 0.25);
    }

    #[test]
    fn clamps_to_endpoints_and_handles_non_finite_sample_times() {
        let curve = Curve::linear(2.0, 8.0);

        assert_eq!(curve.sample(-1.0), 2.0);
        assert_eq!(curve.sample(4.0), 8.0);
        assert_eq!(curve.sample(f32::NAN), 2.0);
        assert_eq!(curve.sample(f32::INFINITY), 2.0);
    }

    #[test]
    fn uses_binary_search_for_non_uniform_keyframe_times() {
        let curve = Curve::new(vec![
            Keyframe::new(0.0, 0.0, Interpolation::Linear),
            Keyframe::new(0.25, 10.0, Interpolation::Linear),
            Keyframe::new(1.0, 40.0, Interpolation::Linear),
        ])
        .expect("valid keyframes");

        assert_eq!(curve.sample(0.625), 25.0);
    }

    #[test]
    fn interpolates_colors_and_vectors() {
        let color = Curve::linear(Color::BLACK, Color::WHITE).sample(0.5);
        let vector = Curve::linear(Vec2::ZERO, Vec2::new(4.0, 6.0)).sample(0.5);

        assert_eq!(color, Color::rgb(128, 128, 128));
        assert_eq!(vector, Vec2::new(2.0, 3.0));
    }

    #[test]
    fn rejects_empty_non_finite_and_unsorted_keyframes() {
        assert_eq!(Curve::<f32>::new(vec![]), Err(CurveError::Empty));
        assert_eq!(
            Curve::new(vec![Keyframe::new(f32::NAN, 1.0, Interpolation::Linear)]),
            Err(CurveError::NonFiniteValue)
        );
        assert_eq!(
            Curve::new(vec![
                Keyframe::new(1.0, 1.0, Interpolation::Linear),
                Keyframe::new(1.0, 2.0, Interpolation::Linear),
            ]),
            Err(CurveError::NonIncreasingTime)
        );
    }
}
