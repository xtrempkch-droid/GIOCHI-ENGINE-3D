//! Audio playback on top of SDL2's mixer.

pub mod mixer;
pub mod sound;

pub use mixer::{mixer_version, AudioSystem};
pub use sound::{
    clamp_volume, MusicHandle, MusicState, SoundHandle, CHUNK_SIZE, LOOPS_INFINITE, MAX_VOLUME,
    MIXING_CHANNELS, SAMPLE_RATE,
};
