//! Sound effects, music and the master volume.
//!
//! The mixer is borrowed by the engine during a frame, so these functions work
//! inside `update` and `on_enter`/`on_exit` (both of which run outside the frame
//! borrow) and outside a run, but not inside `render`: a sound triggered while
//! drawing would play one frame late anyway.
//!
//! A machine without an audio device runs in silent mode (ADR-021): loading and
//! playing return `GIOCHI_ERROR_AUDIO` while the rest of the engine keeps working.

use core::ffi::c_char;

use giochi_engine::error::codes;

use crate::state::{self, guard};

const AUDIO_HINT: &str = "audio is only available inside an update or on_enter/on_exit callback \
     (or outside a run)";

/// Loads a sound effect (WAV, OGG, FLAC, …), fully decoded.
///
/// Returns `0` on failure; `GIOCHI_ERROR_AUDIO` means the mixer is unavailable.
///
/// # C ABI
///
/// ```c
/// GiochiSound giochi_sound_load(const char* path);
/// ```
#[no_mangle]
pub extern "C" fn giochi_sound_load(path: *const c_char) -> giochi_engine::SoundHandle {
    guard(0, || {
        let Some(path) = string(path) else {
            return 0;
        };
        match state::with_audio(AUDIO_HINT, |audio| audio.load_sound(&path)) {
            Ok(Ok(handle)) => handle,
            Ok(Err(err)) => {
                state::fail(&err);
                0
            }
            Err(_) => 0,
        }
    })
}

/// Releases a sound effect.
///
/// Returns `GIOCHI_ERROR_INVALID_HANDLE` for `0` or an already-destroyed handle.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_sound_destroy(GiochiSound sound);
/// ```
#[no_mangle]
pub extern "C" fn giochi_sound_destroy(sound: giochi_engine::SoundHandle) -> i32 {
    guard(codes::INTERNAL, || {
        match state::with_audio(AUDIO_HINT, |audio| audio.destroy_sound(sound)) {
            Ok(true) => codes::OK,
            Ok(false) => {
                state::set_last_error(&format!(
                    "sound handle {sound} is not live (0 or already destroyed)"
                ));
                codes::INVALID_HANDLE
            }
            Err(code) => code,
        }
    })
}

/// Plays a sound effect.
///
/// `loops` is the number of *extra* repetitions: `0` plays once, `-1` loops forever,
/// anything below `-1` is treated as `-1`. More than sixteen simultaneous effects
/// steals the oldest channel (SDL_mixer's behaviour).
///
/// # C ABI
///
/// ```c
/// int32_t giochi_sound_play(GiochiSound sound, int32_t loops);
/// ```
#[no_mangle]
pub extern "C" fn giochi_sound_play(sound: giochi_engine::SoundHandle, loops: i32) -> i32 {
    guard(codes::INTERNAL, || {
        match state::with_audio(AUDIO_HINT, |audio| audio.play_sound(sound, loops)) {
            Ok(Ok(())) => codes::OK,
            Ok(Err(err)) => state::fail(&err),
            Err(code) => code,
        }
    })
}

/// Sets the master volume, clamped to `0..=128`. Out-of-range values are not an
/// error.
///
/// # C ABI
///
/// ```c
/// void giochi_audio_set_volume(int32_t volume);
/// ```
#[no_mangle]
pub extern "C" fn giochi_audio_set_volume(volume: i32) {
    guard((), || {
        let _ = state::with_audio(AUDIO_HINT, |audio| audio.set_volume(volume));
    })
}

/// Returns the master volume.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_audio_volume(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_audio_volume() -> i32 {
    guard(0, || {
        state::with_audio(AUDIO_HINT, |audio| audio.volume()).unwrap_or(0)
    })
}

/// Returns `1` when a working audio device is open.
///
/// Check this once at start-up instead of provoking a failure: a `0` here means the
/// engine is in silent mode and every load/play will fail with `GIOCHI_ERROR_AUDIO`.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_audio_available(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_audio_available() -> i32 {
    guard(0, || {
        state::with_audio(AUDIO_HINT, |audio| audio.is_available())
            .map(i32::from)
            .unwrap_or(0)
    })
}

/// Loads streamed music (OGG, MP3, WAV, …).
///
/// # C ABI
///
/// ```c
/// GiochiMusic giochi_music_load(const char* path);
/// ```
#[no_mangle]
pub extern "C" fn giochi_music_load(path: *const c_char) -> giochi_engine::MusicHandle {
    guard(0, || {
        let Some(path) = string(path) else {
            return 0;
        };
        match state::with_audio(AUDIO_HINT, |audio| audio.load_music(&path)) {
            Ok(Ok(handle)) => handle,
            Ok(Err(err)) => {
                state::fail(&err);
                0
            }
            Err(_) => 0,
        }
    })
}

/// Releases a music handle. Playback stops first, so the mixer never holds a freed
/// stream.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_music_destroy(GiochiMusic music);
/// ```
#[no_mangle]
pub extern "C" fn giochi_music_destroy(music: giochi_engine::MusicHandle) -> i32 {
    guard(codes::INTERNAL, || {
        match state::with_audio(AUDIO_HINT, |audio| audio.destroy_music(music)) {
            Ok(true) => codes::OK,
            Ok(false) => {
                state::set_last_error(&format!(
                    "music handle {music} is not live (0 or already destroyed)"
                ));
                codes::INVALID_HANDLE
            }
            Err(code) => code,
        }
    })
}

/// Plays (or resumes) `music`. `loops` follows the same rule as
/// [`giochi_sound_play`].
///
/// # C ABI
///
/// ```c
/// int32_t giochi_music_play(GiochiMusic music, int32_t loops);
/// ```
#[no_mangle]
pub extern "C" fn giochi_music_play(music: giochi_engine::MusicHandle, loops: i32) -> i32 {
    guard(codes::INTERNAL, || {
        match state::with_audio(AUDIO_HINT, |audio| audio.play_music(music, loops)) {
            Ok(Ok(())) => codes::OK,
            Ok(Err(err)) => state::fail(&err),
            Err(code) => code,
        }
    })
}

/// Pauses the music that is playing.
///
/// # C ABI
///
/// ```c
/// void giochi_music_pause(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_music_pause() {
    guard((), || {
        let _ = state::with_audio(AUDIO_HINT, |audio| audio.pause_music());
    })
}

/// Resumes the paused music.
///
/// # C ABI
///
/// ```c
/// void giochi_music_resume(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_music_resume() {
    guard((), || {
        let _ = state::with_audio(AUDIO_HINT, |audio| audio.resume_music());
    })
}

/// Stops the music.
///
/// # C ABI
///
/// ```c
/// void giochi_music_stop(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_music_stop() {
    guard((), || {
        let _ = state::with_audio(AUDIO_HINT, |audio| audio.stop_music());
    })
}

/// Returns `1` while music is playing.
///
/// # C ABI
///
/// ```c
/// int32_t giochi_music_is_playing(void);
/// ```
#[no_mangle]
pub extern "C" fn giochi_music_is_playing() -> i32 {
    guard(0, || {
        state::with_audio(AUDIO_HINT, |audio| audio.is_music_playing())
            .map(i32::from)
            .unwrap_or(0)
    })
}

/// Reads a NUL-terminated path, reporting a NULL/invalid argument.
fn string(pointer: *const c_char) -> Option<String> {
    match state::cstr(pointer) {
        Some(text) => Some(text),
        None => {
            state::set_last_error("path must be a non-NULL, valid UTF-8 string");
            None
        }
    }
}
