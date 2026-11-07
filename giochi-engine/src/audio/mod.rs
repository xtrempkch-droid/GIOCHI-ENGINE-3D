//! Audio playback on top of SDL2's mixer.

pub mod mixer;
pub mod sound;

pub use mixer::AudioSystem;
pub use sound::{MusicHandle, SoundHandle, LOOPS_INFINITE, MAX_VOLUME};
