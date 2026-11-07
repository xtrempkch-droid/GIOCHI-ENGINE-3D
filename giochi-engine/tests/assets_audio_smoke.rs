//! Headless integration test for the asset loader, the resource cache and audio.
//!
//! Runs against SDL's `dummy` video driver. Audio may or may not have a device in
//! CI; both branches are legitimate and are asserted accordingly.
//!
//! One test per binary: SDL allows a single `EventPump` per process.

use giochi_engine::assets::{asset_size, read_asset, MAX_ASSET_SIZE};
use giochi_engine::audio::{AudioSystem, MusicState, MAX_VOLUME};
use giochi_engine::{codes, Platform, RenderContext, WindowConfig};

fn asset(name: &str) -> String {
    format!("{}/../assets/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn assets_and_audio_smoke_without_a_display() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");

    let platform = Platform::new().expect("dummy driver must initialise");

    // --- loader -----------------------------------------------------------
    let player_path = asset("player.png");
    let size = asset_size(&player_path).expect("player.png must exist");
    assert!(size > 0 && size < MAX_ASSET_SIZE);

    let mut buffer = vec![0u8; size];
    assert_eq!(
        read_asset(&player_path, &mut buffer).expect("read into buffer"),
        size
    );
    // PNG magic, i.e. the bytes really came from the file.
    assert_eq!(&buffer[..4], b"\x89PNG");

    // A buffer that is too small must fail, not truncate.
    let mut small = [0u8; 4];
    assert_eq!(
        read_asset(&player_path, &mut small)
            .expect_err("short buffer must fail")
            .code(),
        codes::ASSET
    );

    // --- resource cache: two loads, one upload ----------------------------
    let window = platform
        .create_window(WindowConfig::new(64, 64), "giochi-assets-test")
        .expect("window must be created");
    let mut render = RenderContext::new(window).expect("renderer must be created");

    let first = render.load_texture(&player_path).expect("first load");
    let second = render.load_texture(&player_path).expect("second load");

    assert_eq!(first, second, "the cache must return the same handle");
    assert_eq!(
        render.texture_count(),
        1,
        "two loads of one path must upload exactly one texture"
    );
    assert_eq!(render.cached_texture_count(), 1);
    assert_eq!(render.cached_texture(&player_path), Some(first));

    // A different path is a second entry.
    let checker_path = asset("checker.png");
    let checker = render.load_texture(&checker_path).expect("checker load");
    assert_ne!(checker, first);
    assert_eq!(render.texture_count(), 2);
    assert_eq!(render.cached_texture_count(), 2);

    // Destroying a texture behind the cache's back must not hand out a dead
    // handle: the next load re-creates it.
    assert!(render.destroy_texture(first));
    assert_eq!(render.cached_texture(&player_path), None);
    let reloaded = render.load_texture(&player_path).expect("reload");
    assert_ne!(reloaded, first, "handles are never reused");
    assert!(render.textures().contains(reloaded));

    render.clear_resource_cache();
    assert_eq!(render.texture_count(), 0, "no texture may leak");
    assert_eq!(render.cached_texture_count(), 0);

    // --- audio ------------------------------------------------------------
    let mut audio = AudioSystem::new(&platform).expect("audio construction must not fail");
    let available = audio.is_available();
    println!("audio device available: {available}");

    if available {
        // A missing file is still an asset error with a real device.
        assert_eq!(
            audio
                .load_sound(&asset("nonexistent.wav"))
                .expect_err("must fail")
                .code(),
            codes::ASSET
        );

        // A real WAV loads, plays and can be destroyed. The file is synthesised
        // here so the test needs no committed binary asset.
        let wav = write_tone_wav();
        let sound = audio.load_sound(wav.to_str().unwrap()).expect("load tone");
        assert!(audio.has_sound(sound));
        assert_eq!(audio.sound_count(), 1);

        audio.play_sound(sound, 0).expect("play tone");
        audio.pause_music();
        assert_eq!(audio.music_state(), MusicState::Stopped);

        assert!(audio.destroy_sound(sound));
        assert!(!audio.destroy_sound(sound), "double destroy is reported");
        assert_eq!(audio.sound_count(), 0);

        // Music follows the same handle lifecycle; an empty path is rejected
        // before touching SDL.
        assert_eq!(
            audio.load_music("").expect_err("must fail").code(),
            codes::INVALID_ARGUMENT
        );
        assert!(!audio.destroy_music(0));
        assert_eq!(audio.music_count(), 0);

        let _ = std::fs::remove_file(&wav);
    } else {
        // Silent mode: playback is refused with a clear audio error, and nothing
        // panics.
        assert_eq!(
            audio
                .load_sound(&asset("nonexistent.wav"))
                .expect_err("must fail")
                .code(),
            codes::AUDIO
        );
    }

    audio.set_volume(999);
    assert_eq!(audio.volume(), MAX_VOLUME);
    audio.set_volume(-1);
    assert_eq!(audio.volume(), 0);

    audio.clear();
    assert_eq!(audio.sound_count(), 0);
    assert_eq!(audio.music_count(), 0);
}

/// Writes a 100 ms 16-bit stereo WAV tone to a temporary file.
fn write_tone_wav() -> std::path::PathBuf {
    const SAMPLE_RATE: u32 = 44_100;
    const FRAMES: u32 = SAMPLE_RATE / 10;

    let mut samples = Vec::with_capacity(FRAMES as usize * 4);
    for frame in 0..FRAMES {
        let t = frame as f32 / SAMPLE_RATE as f32;
        let value = (t * 440.0 * std::f32::consts::TAU).sin() * 0.2;
        let sample = (value * f32::from(i16::MAX)) as i16;
        samples.extend_from_slice(&sample.to_le_bytes());
        samples.extend_from_slice(&sample.to_le_bytes());
    }

    let data_len = samples.len() as u32;
    let mut wav = Vec::with_capacity(44 + samples.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&samples);

    let path = std::env::temp_dir().join(format!("giochi-test-tone-{}.wav", std::process::id()));
    std::fs::write(&path, wav).expect("write tone");
    path
}
