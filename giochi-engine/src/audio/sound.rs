//! Audio handles and playback constants.

/// Opaque sound handle (short, fully decoded effect). `0` is invalid/null.
///
/// # C ABI
///
/// ```c
/// typedef uint64_t GiochiSound;
/// ```
pub type SoundHandle = u64;

/// Opaque music handle (streamed from disk). `0` is invalid/null.
///
/// # C ABI
///
/// ```c
/// typedef uint64_t GiochiMusic;
/// ```
pub type MusicHandle = u64;

/// Looping argument meaning "repeat forever".
pub const LOOPS_INFINITE: i32 = -1;

/// Maximum mixer volume: `0` is mute, [`MAX_VOLUME`] is full scale.
pub const MAX_VOLUME: i32 = 128;

/// Sample rate the engine requests when opening the audio device, in Hz.
pub const SAMPLE_RATE: i32 = 44_100;

/// Audio buffer size requested from the mixer, in sample frames.
///
/// 1024 frames is ~23 ms at 44.1 kHz: low enough for gameplay feedback, large
/// enough to avoid dropouts on modest console hardware.
pub const CHUNK_SIZE: i32 = 1024;

/// Number of mixing channels reserved for sound effects.
pub const MIXING_CHANNELS: i32 = 16;

/// Playback state of the music stream.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MusicState {
    /// Nothing loaded or nothing started.
    #[default]
    Stopped = 0,
    /// A track is playing.
    Playing = 1,
    /// A track is loaded and paused.
    Paused = 2,
}

/// Clamps a caller-supplied volume into the mixer's `0..=MAX_VOLUME` range.
pub fn clamp_volume(volume: i32) -> i32 {
    volume.clamp(0, MAX_VOLUME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_is_clamped_into_the_mixer_range() {
        assert_eq!(clamp_volume(-10), 0);
        assert_eq!(clamp_volume(0), 0);
        assert_eq!(clamp_volume(64), 64);
        assert_eq!(clamp_volume(MAX_VOLUME), MAX_VOLUME);
        assert_eq!(clamp_volume(MAX_VOLUME + 1), MAX_VOLUME);
        assert_eq!(clamp_volume(i32::MAX), MAX_VOLUME);
    }

    #[test]
    fn the_default_music_state_is_stopped() {
        assert_eq!(MusicState::default(), MusicState::Stopped);
        assert_eq!(MusicState::Stopped as i32, 0);
        assert_eq!(MusicState::Playing as i32, 1);
        assert_eq!(MusicState::Paused as i32, 2);
    }

    #[test]
    fn the_loop_and_volume_conventions_match_the_c_contract() {
        assert_eq!(LOOPS_INFINITE, -1);
        assert_eq!(MAX_VOLUME, 128);
        assert_eq!(SAMPLE_RATE, 44_100);
    }

    // Compile-time sanity for the values that would silently break the mixer.
    const _: () = assert!(CHUNK_SIZE > 0);
    const _: () = assert!(MIXING_CHANNELS > 0);
}
