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
