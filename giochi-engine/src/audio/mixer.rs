//! SDL2 mixer wrapper.

use crate::audio::sound::{MusicHandle, SoundHandle};
use crate::error::GiochiError;

/// Owns the SDL2 mixer, every loaded sound chunk and the currently playing music
/// stream.
///
/// # C ABI
///
/// Not exposed. C consumers use `giochi_sound_*`, `giochi_music_*` and
/// `giochi_audio_set_volume`, which act on the engine's single audio system.
#[repr(C)]
#[derive(Debug)]
pub struct AudioSystem {
    _reserved: [u32; 4],
}

impl AudioSystem {
    /// Opens the audio device and initialises SDL2_mixer.
    ///
    /// Implemented in phase 4.
    pub fn new() -> Result<AudioSystem, GiochiError> {
        todo!("fase 4: inicializar SDL2_mixer")
    }

    /// Loads a WAV/OGG/MP3 effect and returns its handle.
    ///
    /// Implemented in phase 4.
    pub fn load_sound(&mut self, _path: &str) -> Result<SoundHandle, GiochiError> {
        todo!("fase 4: carregar som")
    }

    /// Releases the sound behind `handle`.
    ///
    /// Implemented in phase 4.
    pub fn destroy_sound(&mut self, _sound: SoundHandle) -> bool {
        todo!("fase 4: destruir som")
    }

    /// Plays `sound` on the first free channel. `loops` is the number of extra
    /// repetitions, or [`crate::audio::LOOPS_INFINITE`].
    ///
    /// Implemented in phase 4.
    pub fn play_sound(&mut self, _sound: SoundHandle, _loops: i32) -> Result<(), GiochiError> {
        todo!("fase 4: tocar som")
    }

    /// Loads a streamed music track and returns its handle.
    ///
    /// Implemented in phase 4.
    pub fn load_music(&mut self, _path: &str) -> Result<MusicHandle, GiochiError> {
        todo!("fase 4: carregar musica")
    }

    /// Releases the music track behind `handle`.
    ///
    /// Implemented in phase 4.
    pub fn destroy_music(&mut self, _music: MusicHandle) -> bool {
        todo!("fase 4: destruir musica")
    }

    /// Starts playing `music`.
    ///
    /// Implemented in phase 4.
    pub fn play_music(&mut self, _music: MusicHandle, _loops: i32) -> Result<(), GiochiError> {
        todo!("fase 4: tocar musica")
    }

    /// Pauses the current music stream.
    ///
    /// Implemented in phase 4.
    pub fn pause_music(&mut self) {
        todo!("fase 4: pausar musica")
    }

    /// Resumes the current music stream.
    ///
    /// Implemented in phase 4.
    pub fn resume_music(&mut self) {
        todo!("fase 4: retomar musica")
    }

    /// Stops the current music stream.
    ///
    /// Implemented in phase 4.
    pub fn stop_music(&mut self) {
        todo!("fase 4: parar musica")
    }

    /// Sets the master volume, clamped to `0..=MAX_VOLUME`.
    ///
    /// Implemented in phase 4.
    pub fn set_volume(&mut self, _volume: i32) {
        todo!("fase 4: definir volume")
    }

    /// Returns `true` while a music stream is playing.
    ///
    /// Implemented in phase 4.
    pub fn is_music_playing(&self) -> bool {
        todo!("fase 4: estado da musica")
    }
}
