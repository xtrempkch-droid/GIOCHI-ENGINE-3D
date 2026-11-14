//! Sprite animation, easing, tweens and event-driven animation states.
//!
//! # Examples
//!
//! ```no_run
//! use giochi_engine::animation::{AnimationStateMachine, Easing, Tween};
//! use giochi_engine::Vec2;
//!
//! let mut position = Tween::new(
//!     Vec2::ZERO,
//!     Vec2::new(100.0, 0.0),
//!     1.0,
//!     Easing::EaseInOut,
//! )?;
//! let mut states = AnimationStateMachine::new(0);
//! states.add_transition(0, 1, 1)?;
//! position.update(1.0 / 60.0)?;
//! # Ok::<(), giochi_engine::animation::AnimationError>(())
//! ```

mod easing;
mod sprite;
mod state_machine;
mod tween;

use thiserror::Error;

pub use easing::Easing;
pub use sprite::{AnimationFrame, PlaybackMode, SpriteAnimation};
pub use state_machine::AnimationStateMachine;
pub use tween::Tween;

/// Largest number of frames accepted by one sprite animation.
pub const MAX_ANIMATION_FRAMES: usize = 100_000;
/// Largest number of event transitions accepted by one state machine.
pub const MAX_STATE_TRANSITIONS: usize = 4_096;

/// Errors returned by animation, tween and state-machine operations.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::animation::{AnimationError, AnimationFrame};
/// use giochi_engine::Rect;
///
/// let error = AnimationFrame::new(Rect::new(0.0, 0.0, 8.0, 8.0), 0.0);
/// assert!(matches!(error, Err(AnimationError::InvalidDuration)));
/// ```
#[repr(C)]
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum AnimationError {
    /// A texture handle is zero.
    #[error("a sprite animation needs a valid texture handle")]
    InvalidTexture,
    /// No frames were supplied.
    #[error("a sprite animation needs at least one frame")]
    EmptyFrames,
    /// A frame count is zero or exceeds [`MAX_ANIMATION_FRAMES`].
    #[error("sprite animation frame count is outside the supported range")]
    InvalidFrameCount,
    /// A frame rectangle is negative, empty or non-finite.
    #[error("sprite animation frame rectangle is invalid")]
    InvalidFrame,
    /// A frame or tween duration is non-finite or not positive.
    #[error("animation duration must be finite and positive")]
    InvalidDuration,
    /// A time delta is negative or non-finite.
    #[error("animation delta must be finite and non-negative")]
    InvalidDelta,
    /// An easing input is non-finite.
    #[error("easing progress must be finite")]
    InvalidProgress,
    /// A tween endpoint is non-finite.
    #[error("tween endpoints must be finite")]
    InvalidTweenValue,
    /// A state already has a transition for this event.
    #[error("a state already has a transition for this event")]
    DuplicateTransition,
    /// The state machine reached [`MAX_STATE_TRANSITIONS`].
    #[error("animation state machine transition limit reached")]
    TransitionLimitExceeded,
}
