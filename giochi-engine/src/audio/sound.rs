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

/// A group of sounds whose volume is controlled together.
///
/// Four buses, which is the smallest set that solves the problem the plan asked about: a
/// game needs to turn the music down without touching the engine sound, and needs to mute
/// the menu clicks without muting a crash. A bus is a *multiplier* on top of the master
/// volume, not a replacement for it — turning the master to zero still mutes everything,
/// which is what the volume knob in the settings screen means.
///
/// The ordering is part of the C ABI (`GiochiAudioBus`), so it must not be rearranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Bus {
    /// Everything. Its volume multiplies every other bus.
    #[default]
    Master = 0,
    /// Engine, tyres, collisions.
    Sfx = 1,
    /// The soundtrack and any streamed ambience.
    Music = 2,
    /// Clicks, hovers, countdowns: the sounds a player hears hundreds of times.
    Ui = 3,
}

impl Bus {
    /// Every bus, master first.
    pub const ALL: [Bus; 4] = [Bus::Master, Bus::Sfx, Bus::Music, Bus::Ui];

    /// How many buses there are, for a fixed-size table.
    pub const COUNT: usize = 4;

    /// The bus as an index into `AudioSystem`'s volume table.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// A stable name, for logs, tests and the settings file.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::Sfx => "sfx",
            Self::Music => "music",
            Self::Ui => "ui",
        }
    }

    /// The bus for an index, if any.
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Master),
            1 => Some(Self::Sfx),
            2 => Some(Self::Music),
            3 => Some(Self::Ui),
            _ => None,
        }
    }
}

/// The volume one bus actually plays at, given the master and its own slider.
///
/// Integer arithmetic in the mixer's own `0..=MAX_VOLUME` scale: `SDL_mixer` takes an `i32`
/// per channel and there is no reason to introduce a float scale the C side would have to
/// agree with. Rounded rather than truncated so that `MAX_VOLUME * MAX_VOLUME / MAX_VOLUME`
/// is exactly `MAX_VOLUME` and not one below it — an audible difference at full volume on a
/// quiet device.
pub fn effective_volume(master: i32, bus: i32) -> i32 {
    let master = clamp_volume(master) as i64;
    let bus = clamp_volume(bus) as i64;
    let max = MAX_VOLUME as i64;
    ((master * bus + max / 2) / max) as i32
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
