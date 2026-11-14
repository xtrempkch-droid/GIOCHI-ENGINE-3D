//! Easing functions used by tweens.

use super::AnimationError;

/// Easing function for mapping tween progress from `0.0..=1.0`.
///
/// Values outside that range are clamped. Non-finite values return an error.
///
/// # Examples
///
/// ```
/// use giochi_engine::animation::Easing;
///
/// assert_eq!(Easing::EaseIn.sample(0.5)?, 0.25);
/// assert_eq!(Easing::EaseOut.sample(0.5)?, 0.75);
/// # Ok::<(), giochi_engine::animation::AnimationError>(())
/// ```
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Easing {
    /// Constant speed.
    #[default]
    Linear,
    /// Starts slowly and accelerates.
    EaseIn,
    /// Starts quickly and decelerates.
    EaseOut,
    /// Starts and ends slowly.
    EaseInOut,
}

impl Easing {
    /// Maps progress through this easing curve.
    ///
    /// # Examples
    ///
    /// ```
    /// use giochi_engine::animation::{AnimationError, Easing};
    ///
    /// assert_eq!(Easing::EaseInOut.sample(0.5)?, 0.5);
    /// assert_eq!(
    ///     Easing::Linear.sample(f32::NAN),
    ///     Err(AnimationError::InvalidProgress)
    /// );
    /// # Ok::<(), giochi_engine::animation::AnimationError>(())
    /// ```
    pub fn sample(self, progress: f32) -> Result<f32, AnimationError> {
        if !progress.is_finite() {
            return Err(AnimationError::InvalidProgress);
        }
        Ok(self.sample_finite(progress.clamp(0.0, 1.0)))
    }

    pub(crate) fn sample_finite(self, progress: f32) -> f32 {
        match self {
            Self::Linear => progress,
            Self::EaseIn => progress * progress,
            Self::EaseOut => 1.0 - (1.0 - progress) * (1.0 - progress),
            Self::EaseInOut if progress < 0.5 => 2.0 * progress * progress,
            Self::EaseInOut => 1.0 - (-2.0 * progress + 2.0).powi(2) * 0.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_curves_match_their_endpoints_and_midpoints() {
        for easing in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
        ] {
            assert_eq!(easing.sample(0.0), Ok(0.0));
            assert_eq!(easing.sample(1.0), Ok(1.0));
        }

        assert_eq!(Easing::EaseIn.sample(0.5), Ok(0.25));
        assert_eq!(Easing::EaseOut.sample(0.5), Ok(0.75));
        assert_eq!(Easing::EaseInOut.sample(0.5), Ok(0.5));
    }

    #[test]
    fn easing_clamps_finite_progress_and_rejects_non_finite_values() {
        assert_eq!(Easing::Linear.sample(-1.0), Ok(0.0));
        assert_eq!(Easing::Linear.sample(2.0), Ok(1.0));
        assert_eq!(
            Easing::Linear.sample(f32::INFINITY),
            Err(AnimationError::InvalidProgress)
        );
    }
}
