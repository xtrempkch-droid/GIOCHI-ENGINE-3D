//! SDL2 mixer wrapper.

use sdl2::mixer::{Channel, Chunk, Music};
use std::cell::Cell;

use crate::audio::sound::{
    clamp_volume, MusicHandle, MusicState, SoundHandle, CHUNK_SIZE, MIXING_CHANNELS, SAMPLE_RATE,
};
use crate::error::GiochiError;
use crate::handle_registry::HandleRegistry;

/// Owns the SDL2 mixer, every loaded sound chunk and the currently playing music
/// stream.
///
/// Lifecycle: create it once via [`AudioSystem::new`], load sounds/music, play
/// them, then destroy each handle explicitly. Sounds and music are **not** RAII at
/// the API boundary; the `Drop` impl of `AudioSystem` only releases what is still
/// loaded.
///
/// A machine without an audio device is not fatal: [`AudioSystem::new`] logs a
/// warning and returns a usable system in *silent mode*. Loading and playing then
/// return [`GiochiError::Audio`] with a clear message, while the rest of the game
/// keeps running. This is what makes the engine usable in CI and on a bare
/// console before the sound driver is up.
///
/// # C ABI
///
/// Not exposed. C consumers use `giochi_sound_*`, `giochi_music_*` and
/// `giochi_audio_set_volume`, which act on the engine's single audio system.
#[repr(C)]
pub struct AudioSystem {
    sounds: HandleRegistry<Chunk>,
    music: HandleRegistry<Music<'static>>,
    audio: Option<MixerAudio>,
    volume: Cell<i32>,
    #[cfg(feature = "profiling")]
    profile_ns: Cell<u64>,
    _reserved: [u32; 4],
}

impl std::fmt::Debug for AudioSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioSystem")
            .field("device", &self.is_available())
            .field("sounds", &self.sounds.len())
            .field("music", &self.music.len())
            .field("volume", &self.volume.get())
            .field("state", &self.music_state())
            .finish_non_exhaustive()
    }
}

/// Keeps the audio device open for the system's lifetime.
///
/// Separated from `AudioSystem` so that a silent system simply holds `None`.
struct MixerAudio {
    _audio: sdl2::AudioSubsystem,
    _mixer: MixerContext,
}

/// Accumulates the duration of an audio API call into the system's profile counter.
///
/// The guard borrows only the `Cell`, not the whole system, so the instrumented methods
/// can still use their other fields while it is alive.
#[cfg(feature = "profiling")]
struct AudioTimer<'a> {
    started: std::time::Instant,
    total: &'a Cell<u64>,
}

#[cfg(feature = "profiling")]
impl Drop for AudioTimer<'_> {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        self.total.set(self.total.get().saturating_add(elapsed));
    }
}

impl std::fmt::Debug for MixerAudio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MixerAudio").finish_non_exhaustive()
    }
}

/// `Sdl2MixerContext` must outlive every `Chunk`/`Music`; leaking it keeps
/// SDL_mixer initialised for the process, which is what the engine wants.
struct MixerContext;

impl AudioSystem {
    /// Opens the audio device and initialises SDL2_mixer.
    ///
    /// Never fails because of a missing device: the returned system is in silent
    /// mode instead, and [`AudioSystem::is_available`] reports `false`. Failing
    /// start-up because a CI container has no sound card would make the engine
    /// untestable.
    pub fn new(platform: &crate::platform::Platform) -> Result<AudioSystem, GiochiError> {
        let audio = match open_mixer(platform) {
            Ok(audio) => Some(audio),
            Err(error) => {
                log::warn!(
                    "áudio indisponível ({error}); a engine segue em modo silencioso. \
                     Carregar/tocar som retornará erro."
                );
                None
            }
        };

        let system = AudioSystem {
            sounds: HandleRegistry::new(),
            music: HandleRegistry::new(),
            audio,
            volume: Cell::new(crate::audio::MAX_VOLUME),
            #[cfg(feature = "profiling")]
            profile_ns: Cell::new(0),
            _reserved: [0; 4],
        };
        system.apply_volume();
        Ok(system)
    }

    /// Creates a silent system without touching SDL.
    ///
    /// Every load/play call returns [`GiochiError::Audio`]. Intended for tests and
    /// for consumers that disable audio on purpose.
    pub fn silent() -> AudioSystem {
        AudioSystem {
            sounds: HandleRegistry::new(),
            music: HandleRegistry::new(),
            audio: None,
            volume: Cell::new(crate::audio::MAX_VOLUME),
            #[cfg(feature = "profiling")]
            profile_ns: Cell::new(0),
            _reserved: [0; 4],
        }
    }

    /// Returns `true` when a real audio device is open.
    pub fn is_available(&self) -> bool {
        self.audio.is_some()
    }

    /// Returns and clears the nanoseconds spent inside audio API calls since the last
    /// call.
    ///
    /// This is what the frame profiler folds into
    /// [`FrameStats::audio_ns`](crate::perf::FrameStats::audio_ns). Without the
    /// `profiling` feature it always returns `0`.
    pub fn take_profile_time_ns(&self) -> u64 {
        #[cfg(feature = "profiling")]
        {
            let elapsed = self.profile_ns.get();
            self.profile_ns.set(0);
            elapsed
        }
        #[cfg(not(feature = "profiling"))]
        {
            0
        }
    }

    /// Starts a scope timer for an audio API call.
    #[cfg(feature = "profiling")]
    fn timer(&self) -> AudioTimer<'_> {
        AudioTimer {
            started: std::time::Instant::now(),
            total: &self.profile_ns,
        }
    }

    /// Loads a WAV/OGG/MP3 effect and returns its handle.
    ///
    /// The file is fully decoded into memory, which is what makes short effects
    /// low-latency. Use [`AudioSystem::load_music`] for long tracks.
    pub fn load_sound(&mut self, path: &str) -> Result<SoundHandle, GiochiError> {
        self.require_device()?;
        if path.trim().is_empty() {
            return Err(GiochiError::InvalidArgument("sound path is empty"));
        }

        let chunk = Chunk::from_file(path)
            .map_err(|err| GiochiError::Asset(format!("failed to load sound '{path}': {err}")))?;

        Ok(self.sounds.insert(chunk))
    }

    /// Releases the sound behind `handle`. Returns `false` for an invalid or
    /// already-destroyed handle.
    ///
    /// Every sound-effect channel is halted first, and this is a safety requirement
    /// rather than politeness: SDL_mixer keeps the `Mix_Chunk` pointer for as long as a
    /// channel is playing it, so dropping the chunk mid-playback is a use-after-free in
    /// the mixer thread — audible as garbage and reproducible as a crash. The `sdl2`
    /// crate offers no way to ask *which* chunk a channel holds (`Channel::get_chunk`
    /// returns a `Chunk` whose `PartialEq` also compares the ownership flag, and the raw
    /// pointer is private), so the choice is between halting everything and freeing
    /// memory that is still in use.
    ///
    /// [`AudioSystem::destroy_music`] already shipped with the same reasoning for the one
    /// music stream; this path had been missed.
    pub fn destroy_sound(&mut self, handle: SoundHandle) -> bool {
        if !self.sounds.contains(handle) {
            return false;
        }
        if self.audio.is_some() {
            Channel::all().halt();
        }
        self.sounds.remove(handle)
    }

    /// Plays `sound` on the first free channel.
    ///
    /// `loops` is the number of *extra* repetitions: `0` plays once,
    /// [`crate::audio::LOOPS_INFINITE`] loops forever. Negative values other than
    /// `-1` are clamped to `0`.
    pub fn play_sound(&mut self, sound: SoundHandle, loops: i32) -> Result<(), GiochiError> {
        #[cfg(feature = "profiling")]
        let _timer = self.timer();
        self.require_device()?;
        let chunk = self
            .sounds
            .get(sound)
            .ok_or(GiochiError::InvalidHandle(sound))?;

        Channel::all()
            .play(&chunk, normalise_loops(loops))
            .map(|_| ())
            .map_err(|err| GiochiError::Audio(format!("failed to play sound: {err}")))
    }

    /// Stops every sound effect channel.
    pub fn stop_all_sounds(&mut self) {
        #[cfg(feature = "profiling")]
        let _timer = self.timer();
        if self.audio.is_some() {
            Channel::all().halt();
        }
    }

    /// Loads a streamed music track and returns its handle.
    ///
    /// Music is not decoded into memory, so long tracks cost almost no RAM.
    pub fn load_music(&mut self, path: &str) -> Result<MusicHandle, GiochiError> {
        self.require_device()?;
        if path.trim().is_empty() {
            return Err(GiochiError::InvalidArgument("music path is empty"));
        }

        let music = Music::from_file(path)
            .map_err(|err| GiochiError::Asset(format!("failed to load music '{path}': {err}")))?;

        Ok(self.music.insert(music))
    }

    /// Releases the music track behind `handle`.
    ///
    /// **Playback stops, whatever `handle` is**, and that is a limitation rather than a
    /// choice: SDL_mixer exposes no way to ask which `Mix_Music` the single stream is
    /// playing (`Mix_PlayingMusic` says whether, not what), so the only safe thing to do
    /// before freeing is to halt. Freeing a `Mix_Music` that is still playing is a
    /// use-after-free in the mixer thread. The earlier wording claimed playback stopped
    /// only for the playing track, which the code never did.
    pub fn destroy_music(&mut self, handle: MusicHandle) -> bool {
        if self.music.get(handle).is_some() {
            Music::halt();
        }
        self.music.remove(handle)
    }

    /// Starts playing `music`, replacing whatever was playing.
    ///
    /// `loops` follows the same convention as [`AudioSystem::play_sound`].
    pub fn play_music(&mut self, music: MusicHandle, loops: i32) -> Result<(), GiochiError> {
        #[cfg(feature = "profiling")]
        let _timer = self.timer();
        self.require_device()?;
        let track = self
            .music
            .get(music)
            .ok_or(GiochiError::InvalidHandle(music))?;

        track
            .play(normalise_loops(loops))
            .map_err(|err| GiochiError::Audio(format!("failed to play music: {err}")))
    }

    /// Pauses the current music stream. Resuming continues from the same position.
    pub fn pause_music(&mut self) {
        #[cfg(feature = "profiling")]
        let _timer = self.timer();
        if self.audio.is_some() {
            Music::pause();
        }
    }

    /// Resumes the current music stream.
    pub fn resume_music(&mut self) {
        #[cfg(feature = "profiling")]
        let _timer = self.timer();
        if self.audio.is_some() {
            Music::resume();
        }
    }

    /// Stops the current music stream and rewinds it to the start.
    pub fn stop_music(&mut self) {
        #[cfg(feature = "profiling")]
        let _timer = self.timer();
        if self.audio.is_some() {
            Music::halt();
        }
    }

    /// Sets the master volume, clamped to `0..=MAX_VOLUME`.
    pub fn set_volume(&mut self, volume: i32) {
        #[cfg(feature = "profiling")]
        let _timer = self.timer();
        self.volume.set(clamp_volume(volume));
        self.apply_volume();
    }

    /// Returns the current master volume in `0..=MAX_VOLUME`.
    pub fn volume(&self) -> i32 {
        self.volume.get()
    }

    /// Returns `true` while a music stream is playing (paused counts as not
    /// playing).
    pub fn is_music_playing(&self) -> bool {
        self.audio.is_some() && Music::is_playing() && !Music::is_paused()
    }

    /// Returns the current music state.
    pub fn music_state(&self) -> MusicState {
        if self.audio.is_none() || !Music::is_playing() {
            MusicState::Stopped
        } else if Music::is_paused() {
            MusicState::Paused
        } else {
            MusicState::Playing
        }
    }

    /// Number of live sound handles.
    pub fn sound_count(&self) -> usize {
        self.sounds.len()
    }

    /// Number of live music handles.
    pub fn music_count(&self) -> usize {
        self.music.len()
    }

    /// Returns `true` when `handle` refers to a live sound.
    pub fn has_sound(&self, handle: SoundHandle) -> bool {
        self.sounds.contains(handle)
    }

    /// Returns `true` when `handle` refers to a live music track.
    pub fn has_music(&self, handle: MusicHandle) -> bool {
        self.music.contains(handle)
    }

    /// Destroys every sound and music track.
    pub fn clear(&mut self) {
        self.stop_music();
        self.stop_all_sounds();
        self.sounds.clear();
        self.music.clear();
    }

    fn require_device(&self) -> Result<(), GiochiError> {
        if self.audio.is_some() {
            Ok(())
        } else {
            Err(GiochiError::Audio(
                "no audio device is available (the engine is in silent mode)".to_owned(),
            ))
        }
    }

    fn apply_volume(&self) {
        if self.audio.is_some() {
            let volume = self.volume.get();
            Channel::all().set_volume(volume);
            Music::set_volume(volume);
        }
    }
}

impl Drop for AudioSystem {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Maps the engine's loop convention (`-1` = forever, `n` = n extra repeats) onto
/// SDL_mixer's, which uses the same convention but must not receive `< -1`.
fn normalise_loops(loops: i32) -> i32 {
    if loops < 0 {
        crate::audio::LOOPS_INFINITE
    } else {
        loops
    }
}

/// Opens the audio device and initialises SDL_mixer's decoders.
fn open_mixer(platform: &crate::platform::Platform) -> Result<MixerAudio, String> {
    let audio = platform
        .audio_subsystem()
        .map_err(|error| error.to_string())?;

    // SDL_mixer's OGG/MP3 decoders are optional at build time; request the ones we
    // care about and accept whatever the library actually provides.
    let wanted = sdl2::mixer::InitFlag::OGG | sdl2::mixer::InitFlag::MP3;
    match sdl2::mixer::init(wanted) {
        Ok(context) => {
            // Must outlive every Chunk/Music; leaking keeps the decoders alive for
            // the engine's lifetime.
            std::mem::forget(context);
        }
        Err(err) => {
            log::warn!("SDL_mixer: decodificadores opcionais indisponíveis ({err}); seguindo");
        }
    }

    // SDL_mixer wants the raw `AUDIO_*` bit pattern as a `u16`; `sdl2` only
    // exposes it through `sys`, so the constant is widened and truncated here.
    let format = sdl2::sys::AUDIO_S16LSB as u16;
    sdl2::mixer::open_audio(SAMPLE_RATE, format, 2, CHUNK_SIZE)?;
    sdl2::mixer::allocate_channels(MIXING_CHANNELS);

    Ok(MixerAudio {
        _audio: audio,
        _mixer: MixerContext,
    })
}

/// Returns the dynamically linked SDL_mixer version as `(major, minor, patch)`.
///
/// Useful for diagnostics: the engine bundles SDL_mixer, so this is the library
/// actually loaded by the binary.
pub fn mixer_version() -> (u32, u32, u32) {
    let version = sdl2::mixer::get_linked_version();
    (
        u32::from(version.major),
        u32::from(version.minor),
        u32::from(version.patch),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_silent_system_reports_unavailable_and_rejects_playback() {
        let mut system = AudioSystem::silent();

        assert!(!system.is_available());
        assert_eq!(
            system.load_sound("x.wav").expect_err("must fail").code(),
            crate::error::codes::AUDIO
        );
        assert_eq!(
            system.load_music("x.ogg").expect_err("must fail").code(),
            crate::error::codes::AUDIO
        );
        assert_eq!(
            system.play_sound(1, 0).expect_err("must fail").code(),
            crate::error::codes::AUDIO
        );
        assert_eq!(
            system.play_music(1, 0).expect_err("must fail").code(),
            crate::error::codes::AUDIO
        );
    }

    #[test]
    fn a_silent_system_is_always_stopped() {
        let system = AudioSystem::silent();

        assert!(!system.is_music_playing());
        assert_eq!(system.music_state(), MusicState::Stopped);
    }

    #[test]
    fn volume_is_clamped_and_readable_without_a_device() {
        let mut system = AudioSystem::silent();

        system.set_volume(999);
        assert_eq!(system.volume(), crate::audio::MAX_VOLUME);

        system.set_volume(-5);
        assert_eq!(system.volume(), 0);
    }

    #[test]
    fn destroying_unknown_handles_is_reported() {
        let mut system = AudioSystem::silent();

        assert!(!system.destroy_sound(0));
        assert!(!system.destroy_sound(42));
        assert!(!system.destroy_music(0));
        assert!(!system.destroy_music(42));
        assert_eq!(system.sound_count(), 0);
        assert_eq!(system.music_count(), 0);
    }

    #[test]
    fn muting_and_stopping_are_safe_without_a_device() {
        let mut system = AudioSystem::silent();

        system.pause_music();
        system.resume_music();
        system.stop_music();
        system.stop_all_sounds();
        system.clear();

        assert!(system.sound_count() == 0 && system.music_count() == 0);
    }

    #[test]
    fn the_loop_convention_is_normalised_for_sdl() {
        assert_eq!(normalise_loops(-1), -1);
        assert_eq!(normalise_loops(-5), -1);
        assert_eq!(normalise_loops(0), 0);
        assert_eq!(normalise_loops(3), 3);
    }

    #[test]
    fn the_mixer_version_is_queryable() {
        let (major, minor, patch) = mixer_version();

        assert!(
            major >= 2,
            "unexpected SDL_mixer version {major}.{minor}.{patch}"
        );
    }
}
