//! Typed value interpolation over time.

use crate::particles::CurveValue;

use super::{AnimationError, Easing};

/// A finite-duration tween over a [`CurveValue`].
///
/// The tween clamps at its endpoint and does not allocate while it updates.
///
/// # Examples
///
/// ```
/// use giochi_engine::animation::{Easing, Tween};
/// use giochi_engine::Vec2;
///
/// let mut tween = Tween::new(
///     Vec2::ZERO,
///     Vec2::new(10.0, 0.0),
///     1.0,
///     Easing::Linear,
/// )?;
/// assert_eq!(tween.update(0.5)?, Vec2::new(5.0, 0.0));
/// # Ok::<(), giochi_engine::animation::AnimationError>(())
/// ```
#[repr(C)]
#[derive(Debug, Clone, PartialEq)]
pub struct Tween<T> {
    start: T,
    end: T,
    duration: f32,
    elapsed: f32,
    easing: Easing,
    _reserved: [u32; 4],
}

impl<T: CurveValue> Tween<T> {
    /// Creates a tween with finite endpoints and a positive duration.
    ///
    /// # Examples
    ///
    /// ```
    /// use giochi_engine::animation::{Easing, Tween};
    ///
    /// let tween = Tween::new(2.0, 6.0, 2.0, Easing::EaseIn)?;
    /// assert_eq!(tween.value(), 2.0);
    /// # Ok::<(), giochi_engine::animation::AnimationError>(())
    /// ```
    pub fn new(start: T, end: T, duration: f32, easing: Easing) -> Result<Self, AnimationError> {
        if !start.is_finite() || !end.is_finite() {
            return Err(AnimationError::InvalidTweenValue);
        }
        if !duration.is_finite() || duration <= 0.0 {
            return Err(AnimationError::InvalidDuration);
        }

        Ok(Self {
            start,
            end,
            duration,
            elapsed: 0.0,
            easing,
            _reserved: [0; 4],
        })
    }

    /// Advances by `delta` seconds and returns the interpolated value.
    ///
    /// # Examples
    ///
    /// ```
    /// use giochi_engine::animation::{Easing, Tween};
    ///
    /// let mut tween = Tween::new(0.0, 1.0, 2.0, Easing::Linear)?;
    /// assert_eq!(tween.update(1.0)?, 0.5);
    /// # Ok::<(), giochi_engine::animation::AnimationError>(())
    /// ```
    pub fn update(&mut self, delta: f32) -> Result<T, AnimationError> {
        if !delta.is_finite() || delta < 0.0 {
            return Err(AnimationError::InvalidDelta);
        }
        self.elapsed = (self.elapsed + delta).min(self.duration);
        Ok(self.value())
    }

    /// The value at the current elapsed time.
    ///
    /// # Examples
    ///
    /// ```
    /// use giochi_engine::animation::{Easing, Tween};
    ///
    /// let tween = Tween::new(3.0, 9.0, 1.0, Easing::Linear)?;
    /// assert_eq!(tween.value(), 3.0);
    /// # Ok::<(), giochi_engine::animation::AnimationError>(())
    /// ```
    pub fn value(&self) -> T {
        let progress = (self.elapsed / self.duration).clamp(0.0, 1.0);
        self.start
            .interpolate(self.end, self.easing.sample_finite(progress))
    }

    /// Restarts the tween at its initial value.
    ///
    /// # Examples
    ///
    /// ```
    /// use giochi_engine::animation::{Easing, Tween};
    ///
    /// let mut tween = Tween::new(0.0, 1.0, 1.0, Easing::Linear)?;
    /// tween.update(1.0)?;
    /// tween.reset();
    /// assert_eq!(tween.value(), 0.0);
    /// # Ok::<(), giochi_engine::animation::AnimationError>(())
    /// ```
    pub fn reset(&mut self) {
        self.elapsed = 0.0;
    }

    /// Elapsed time in seconds.
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    /// Total duration in seconds.
    pub fn duration(&self) -> f32 {
        self.duration
    }

    /// Normalised progress in `0.0..=1.0`.
    pub fn progress(&self) -> f32 {
        (self.elapsed / self.duration).clamp(0.0, 1.0)
    }

    /// Whether the tween has reached its endpoint.
    pub fn is_finished(&self) -> bool {
        self.elapsed >= self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Color, Vec2};

    #[test]
    fn tween_interpolates_and_clamps_at_its_endpoint() {
        let mut tween = Tween::new(0.0, 10.0, 2.0, Easing::Linear).expect("valid tween");

        assert_eq!(tween.update(1.0), Ok(5.0));
        assert_eq!(tween.update(10.0), Ok(10.0));
        assert_eq!(tween.value(), 10.0);
        assert!(tween.is_finished());
    }

    #[test]
    fn tween_applies_easing_and_supports_vectors_and_colors() {
        let mut eased = Tween::new(0.0, 1.0, 1.0, Easing::EaseIn).expect("valid tween");
        assert_eq!(eased.update(0.5), Ok(0.25));

        let mut position = Tween::new(Vec2::ZERO, Vec2::new(8.0, 4.0), 1.0, Easing::Linear)
            .expect("valid vector tween");
        assert_eq!(position.update(0.5), Ok(Vec2::new(4.0, 2.0)));

        let mut color = Tween::new(
            Color::rgb(0, 20, 40),
            Color::rgb(100, 120, 140),
            1.0,
            Easing::Linear,
        )
        .expect("valid color tween");
        assert_eq!(color.update(0.5), Ok(Color::rgb(50, 70, 90)));
    }

    #[test]
    fn tween_validates_inputs_and_can_be_restarted() {
        assert!(matches!(
            Tween::new(0.0, 1.0, 0.0, Easing::Linear),
            Err(AnimationError::InvalidDuration)
        ));
        assert!(matches!(
            Tween::new(f32::NAN, 1.0, 1.0, Easing::Linear),
            Err(AnimationError::InvalidTweenValue)
        ));

        let mut tween = Tween::new(1.0, 3.0, 1.0, Easing::Linear).expect("valid tween");
        assert_eq!(tween.update(-0.1), Err(AnimationError::InvalidDelta));
        assert_eq!(tween.update(f32::NAN), Err(AnimationError::InvalidDelta));
        tween.update(1.0).expect("valid delta");
        tween.reset();
        assert_eq!(tween.value(), 1.0);
        assert_eq!(tween.progress(), 0.0);
    }
}
