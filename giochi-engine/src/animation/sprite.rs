//! Sprite-sheet animation.

use crate::error::GiochiError;
use crate::math::{Rect, Vec2};
use crate::render::{RenderContext, Sprite, TextureHandle};

use super::{AnimationError, MAX_ANIMATION_FRAMES};

/// Playback behavior after the last frame.
///
/// # Examples
///
/// ```
/// use giochi_engine::animation::PlaybackMode;
///
/// let mode = PlaybackMode::PingPong;
/// assert_eq!(mode, PlaybackMode::PingPong);
/// ```
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackMode {
    /// Stop on the last frame.
    Once,
    /// Wrap from the last frame to the first.
    #[default]
    Loop,
    /// Play forward, then backward without repeating the endpoints.
    PingPong,
}

/// Source rectangle and display duration for one sprite frame.
///
/// The rectangle is measured in texture pixels.
///
/// # Examples
///
/// ```
/// use giochi_engine::animation::AnimationFrame;
/// use giochi_engine::Rect;
///
/// let frame = AnimationFrame::new(Rect::new(0.0, 0.0, 16.0, 16.0), 0.1)?;
/// assert_eq!(frame.duration, 0.1);
/// # Ok::<(), giochi_engine::animation::AnimationError>(())
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationFrame {
    /// Source rectangle in texture pixels.
    pub source: Rect,
    /// Duration in seconds.
    pub duration: f32,
    _reserved: [u32; 4],
}

impl AnimationFrame {
    /// Creates a frame with a finite, non-empty source rectangle and positive duration.
    ///
    /// # Examples
    ///
    /// ```
    /// use giochi_engine::animation::AnimationFrame;
    /// use giochi_engine::Rect;
    ///
    /// let frame = AnimationFrame::new(Rect::new(16.0, 0.0, 16.0, 16.0), 0.08)?;
    /// assert_eq!(frame.source.x, 16.0);
    /// # Ok::<(), giochi_engine::animation::AnimationError>(())
    /// ```
    pub fn new(source: Rect, duration: f32) -> Result<Self, AnimationError> {
        validate_frame(source, duration)?;
        Ok(Self {
            source,
            duration,
            _reserved: [0; 4],
        })
    }
}

/// Plays texture regions as a sprite animation.
///
/// `update` advances by seconds. Drawing converts the destination rectangle from world
/// space through the renderer's camera and keeps the selected source region in the normal
/// sprite batch.
///
/// # Examples
///
/// ```no_run
/// use giochi_engine::animation::{AnimationFrame, PlaybackMode, SpriteAnimation};
/// use giochi_engine::{Rect, RenderContext};
///
/// let frames = vec![
///     AnimationFrame::new(Rect::new(0.0, 0.0, 16.0, 16.0), 0.1)?,
///     AnimationFrame::new(Rect::new(16.0, 0.0, 16.0, 16.0), 0.1)?,
/// ];
/// let mut animation = SpriteAnimation::new(1, frames, PlaybackMode::Loop)?;
/// animation.update(1.0 / 60.0)?;
/// # let _ = &mut animation;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[repr(C)]
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteAnimation {
    texture: TextureHandle,
    frames: Vec<AnimationFrame>,
    current_frame: usize,
    elapsed: f32,
    playing: bool,
    finished: bool,
    direction: i8,
    mode: PlaybackMode,
    cycle_duration: f32,
    _reserved: [u32; 4],
}

impl SpriteAnimation {
    /// Creates an animation from explicit source rectangles and per-frame durations.
    ///
    /// # Examples
    ///
    /// ```
    /// use giochi_engine::animation::{AnimationFrame, PlaybackMode, SpriteAnimation};
    /// use giochi_engine::Rect;
    ///
    /// let frames = vec![AnimationFrame::new(Rect::new(0.0, 0.0, 8.0, 8.0), 0.2)?];
    /// let animation = SpriteAnimation::new(3, frames, PlaybackMode::Once)?;
    /// assert_eq!(animation.frame_count(), 1);
    /// # Ok::<(), giochi_engine::animation::AnimationError>(())
    /// ```
    pub fn new(
        texture: TextureHandle,
        frames: Vec<AnimationFrame>,
        mode: PlaybackMode,
    ) -> Result<Self, AnimationError> {
        if texture == 0 {
            return Err(AnimationError::InvalidTexture);
        }
        if frames.is_empty() {
            return Err(AnimationError::EmptyFrames);
        }
        if frames.len() > MAX_ANIMATION_FRAMES {
            return Err(AnimationError::InvalidFrameCount);
        }

        let mut total_duration = 0.0;
        for frame in &frames {
            validate_frame(frame.source, frame.duration)?;
            total_duration += frame.duration;
            if !total_duration.is_finite() {
                return Err(AnimationError::InvalidDuration);
            }
        }

        let cycle_duration = match mode {
            PlaybackMode::Once | PlaybackMode::Loop => total_duration,
            PlaybackMode::PingPong if frames.len() > 1 => {
                let last = frames.len() - 1;
                let interior_duration: f32 =
                    frames[1..last].iter().map(|frame| frame.duration).sum();
                frames[0].duration + frames[last].duration + 2.0 * interior_duration
            }
            PlaybackMode::PingPong => total_duration,
        };
        if !cycle_duration.is_finite() || cycle_duration <= 0.0 {
            return Err(AnimationError::InvalidDuration);
        }

        Ok(Self {
            texture,
            frames,
            current_frame: 0,
            elapsed: 0.0,
            playing: true,
            finished: false,
            direction: 1,
            mode,
            cycle_duration,
            _reserved: [0; 4],
        })
    }

    /// Creates a horizontal strip with equally sized frames.
    ///
    /// The caller is responsible for ensuring the regions fit inside `texture`.
    ///
    /// # Examples
    ///
    /// ```
    /// use giochi_engine::animation::{PlaybackMode, SpriteAnimation};
    /// use giochi_engine::Vec2;
    ///
    /// let animation =
    ///     SpriteAnimation::from_strip(2, Vec2::new(16.0, 16.0), 4, 0.1, PlaybackMode::Loop)?;
    /// assert_eq!(animation.frame_count(), 4);
    /// # Ok::<(), giochi_engine::animation::AnimationError>(())
    /// ```
    pub fn from_strip(
        texture: TextureHandle,
        frame_size: Vec2,
        frame_count: u32,
        frame_duration: f32,
        mode: PlaybackMode,
    ) -> Result<Self, AnimationError> {
        if frame_count == 0 || frame_count as usize > MAX_ANIMATION_FRAMES {
            return Err(AnimationError::InvalidFrameCount);
        }
        if !frame_size.is_finite() || frame_size.x <= 0.0 || frame_size.y <= 0.0 {
            return Err(AnimationError::InvalidFrame);
        }

        let mut frames = Vec::with_capacity(frame_count as usize);
        for index in 0..frame_count {
            let x = frame_size.x * index as f32;
            frames.push(AnimationFrame::new(
                Rect::new(x, 0.0, frame_size.x, frame_size.y),
                frame_duration,
            )?);
        }
        Self::new(texture, frames, mode)
    }

    /// Advances the animation by `delta` seconds.
    ///
    /// Large deltas in looping modes are reduced by the cycle duration, so update time
    /// remains bounded by the number of frames rather than by elapsed wall time.
    ///
    /// # Examples
    ///
    /// ```
    /// use giochi_engine::animation::{AnimationFrame, PlaybackMode, SpriteAnimation};
    /// use giochi_engine::Rect;
    ///
    /// let frames = vec![AnimationFrame::new(Rect::new(0.0, 0.0, 8.0, 8.0), 0.1)?];
    /// let mut animation = SpriteAnimation::new(1, frames, PlaybackMode::Loop)?;
    /// animation.update(0.25)?;
    /// assert_eq!(animation.current_frame_index(), 0);
    /// # Ok::<(), giochi_engine::animation::AnimationError>(())
    /// ```
    pub fn update(&mut self, delta: f32) -> Result<(), AnimationError> {
        if !delta.is_finite() || delta < 0.0 {
            return Err(AnimationError::InvalidDelta);
        }
        if !self.playing || delta == 0.0 {
            return Ok(());
        }

        let mut remaining = if self.mode == PlaybackMode::Once {
            delta
        } else {
            delta.rem_euclid(self.cycle_duration)
        };

        while remaining > 0.0 {
            let until_next = self.frames[self.current_frame].duration - self.elapsed;
            if remaining < until_next {
                self.elapsed += remaining;
                break;
            }

            remaining -= until_next;
            self.elapsed = 0.0;
            if !self.advance_frame() {
                break;
            }
        }
        Ok(())
    }

    /// Starts or resumes playback. A completed one-shot animation must be reset first.
    pub fn play(&mut self) {
        if !self.finished {
            self.playing = true;
        }
    }

    /// Pauses playback without changing the current frame.
    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// Restarts playback from the first frame.
    pub fn reset(&mut self) {
        self.current_frame = 0;
        self.elapsed = 0.0;
        self.playing = true;
        self.finished = false;
        self.direction = 1;
    }

    /// Draws the current frame into a world-space destination rectangle.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use giochi_engine::animation::{AnimationFrame, PlaybackMode, SpriteAnimation};
    /// use giochi_engine::{Rect, RenderContext};
    ///
    /// # fn draw(renderer: &mut RenderContext) -> Result<(), Box<dyn std::error::Error>> {
    /// let frames = vec![AnimationFrame::new(Rect::new(0.0, 0.0, 8.0, 8.0), 0.1)?];
    /// let animation = SpriteAnimation::new(1, frames, PlaybackMode::Loop)?;
    /// animation.draw(renderer, Rect::new(20.0, 30.0, 8.0, 8.0))?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn draw(&self, renderer: &mut RenderContext, destination: Rect) -> Result<(), GiochiError> {
        let sprite = Sprite::new(
            self.texture,
            destination.x,
            destination.y,
            destination.w,
            destination.h,
        );
        renderer.draw_sprite_region(&sprite, self.frames[self.current_frame].source)
    }

    /// Texture handle used by this animation.
    pub fn texture(&self) -> TextureHandle {
        self.texture
    }

    /// Number of frames.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Current frame index, starting at zero.
    pub fn current_frame_index(&self) -> usize {
        self.current_frame
    }

    /// Current frame's source rectangle in texture pixels.
    pub fn current_source(&self) -> Rect {
        self.frames[self.current_frame].source
    }

    /// Whether this animation is currently advancing.
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Whether a one-shot animation reached its last frame.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn advance_frame(&mut self) -> bool {
        match self.mode {
            PlaybackMode::Once => {
                if self.current_frame + 1 == self.frames.len() {
                    self.elapsed = self.frames[self.current_frame].duration;
                    self.finished = true;
                    self.playing = false;
                    false
                } else {
                    self.current_frame += 1;
                    true
                }
            }
            PlaybackMode::Loop => {
                self.current_frame = (self.current_frame + 1) % self.frames.len();
                true
            }
            PlaybackMode::PingPong if self.frames.len() == 1 => true,
            PlaybackMode::PingPong if self.direction > 0 => {
                if self.current_frame + 1 == self.frames.len() {
                    self.direction = -1;
                    self.current_frame -= 1;
                } else {
                    self.current_frame += 1;
                }
                true
            }
            PlaybackMode::PingPong => {
                if self.current_frame == 0 {
                    self.direction = 1;
                    self.current_frame = 1;
                } else {
                    self.current_frame -= 1;
                }
                true
            }
        }
    }
}

fn validate_frame(source: Rect, duration: f32) -> Result<(), AnimationError> {
    if !duration.is_finite() || duration <= 0.0 {
        return Err(AnimationError::InvalidDuration);
    }
    if ![source.x, source.y, source.w, source.h]
        .iter()
        .all(|value| value.is_finite())
        || source.x < 0.0
        || source.y < 0.0
        || source.w <= 0.0
        || source.h <= 0.0
    {
        return Err(AnimationError::InvalidFrame);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(count: usize, duration: f32) -> Vec<AnimationFrame> {
        (0..count)
            .map(|index| {
                AnimationFrame::new(Rect::new(index as f32 * 8.0, 0.0, 8.0, 8.0), duration)
                    .expect("valid frame")
            })
            .collect()
    }

    #[test]
    fn once_stops_on_the_last_frame_and_reset_restarts() {
        let mut animation =
            SpriteAnimation::new(1, frames(2, 0.5), PlaybackMode::Once).expect("valid animation");

        animation.update(0.5).expect("valid delta");
        assert_eq!(animation.current_frame_index(), 1);
        assert!(!animation.is_finished());
        animation.update(0.5).expect("valid delta");
        assert_eq!(animation.current_frame_index(), 1);
        assert!(animation.is_finished());
        assert!(!animation.is_playing());

        animation.reset();
        assert_eq!(animation.current_frame_index(), 0);
        assert!(animation.is_playing());
        assert!(!animation.is_finished());
    }

    #[test]
    fn looping_uses_per_frame_durations_and_handles_large_deltas() {
        let mut animation =
            SpriteAnimation::new(1, frames(3, 0.25), PlaybackMode::Loop).expect("valid animation");

        animation.update(0.625).expect("valid delta");
        assert_eq!(animation.current_frame_index(), 2);
        animation.update(1_000_000.0).expect("large finite delta");
        assert!(animation.current_frame_index() < 3);
    }

    #[test]
    fn ping_pong_does_not_repeat_the_endpoints() {
        let mut animation = SpriteAnimation::new(1, frames(3, 0.5), PlaybackMode::PingPong)
            .expect("valid animation");
        let mut observed = vec![animation.current_frame_index()];

        for _ in 0..5 {
            animation.update(0.5).expect("valid delta");
            observed.push(animation.current_frame_index());
        }

        assert_eq!(observed, [0, 1, 2, 1, 0, 1]);
    }

    #[test]
    fn constructors_reject_invalid_handles_frames_and_durations() {
        assert!(matches!(
            SpriteAnimation::new(0, frames(1, 0.1), PlaybackMode::Loop),
            Err(AnimationError::InvalidTexture)
        ));
        assert!(matches!(
            SpriteAnimation::new(1, Vec::new(), PlaybackMode::Loop),
            Err(AnimationError::EmptyFrames)
        ));
        assert!(matches!(
            AnimationFrame::new(Rect::new(0.0, 0.0, -1.0, 8.0), 0.1),
            Err(AnimationError::InvalidFrame)
        ));
        assert!(matches!(
            AnimationFrame::new(Rect::new(0.0, 0.0, 8.0, 8.0), f32::INFINITY),
            Err(AnimationError::InvalidDuration)
        ));
        assert!(matches!(
            SpriteAnimation::from_strip(1, Vec2::ZERO, 4, 0.1, PlaybackMode::Loop),
            Err(AnimationError::InvalidFrame)
        ));
    }

    #[test]
    fn pause_and_play_preserve_the_current_frame() {
        let mut animation =
            SpriteAnimation::new(1, frames(2, 0.5), PlaybackMode::Loop).expect("valid animation");
        animation.update(0.25).expect("valid delta");
        animation.pause();
        animation.update(1.0).expect("valid delta");
        assert_eq!(animation.current_frame_index(), 0);
        animation.play();
        animation.update(0.25).expect("valid delta");
        assert_eq!(animation.current_frame_index(), 1);
    }
}
