//! End-to-end test of the C ABI, driven from Rust.
//!
//! Everything here calls the exported `extern "C"` functions exactly as a C program
//! would — no access to `giochi-engine` beyond building the callback structs — so a
//! mistake in the boundary (a wrong code, a leaked handle, a stack that is not
//! emptied) fails here rather than in a consumer.
//!
//! One test per binary: SDL allows a single `EventPump` per process.

use std::ffi::{CStr, CString};
use std::os::raw::c_void;

use giochi::*;

/// Counts what the engine called, so the test can assert on the real order.
#[derive(Default)]
struct Probe {
    entered: u32,
    exited: u32,
    updates: u32,
    renders: u32,
    /// Frames after which the scene asks the loop to stop; `0` never quits.
    quit_after: u32,
    /// Set when the callbacks observe a subsystem they should be able to use.
    saw_engine_running: bool,
    saw_texture_ok: bool,
}

/// Absolute path to a committed asset, independent of the working directory.
fn asset(name: &str) -> String {
    format!("{}/../assets/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn message() -> String {
    // SAFETY: the pointer is owned by the library and valid until the next call.
    let text = unsafe { CStr::from_ptr(giochi_get_last_error()) };
    text.to_string_lossy().into_owned()
}

extern "C" fn on_enter(user_data: *mut c_void) {
    let probe = unsafe { &mut *(user_data as *mut Probe) };
    probe.entered += 1;
    // Texture creation must work here: `on_enter` runs outside the engine borrow.
    // A *different* path from the one the test body uses, because destroying a handle
    // that came from the resource cache leaves a stale cache entry (documented in
    // docs/C_ABI.md §6).
    let path = CString::new(asset("checker.png")).unwrap();
    let texture = giochi_texture_create(path.as_ptr());
    probe.saw_texture_ok = texture != 0 && giochi_texture_width(texture) == 64;
    assert_eq!(
        giochi_texture_destroy(texture),
        GIOCHI_ERROR_OK,
        "{}",
        message()
    );
}

extern "C" fn on_exit(user_data: *mut c_void) {
    let probe = unsafe { &mut *(user_data as *mut Probe) };
    probe.exited += 1;
}

extern "C" fn update(user_data: *mut c_void, time: *const giochi_engine::Time) {
    let probe = unsafe { &mut *(user_data as *mut Probe) };
    probe.updates += 1;
    assert!(!time.is_null());
    // SAFETY: the engine owns the value for the duration of the callback.
    let time = unsafe { *time };
    assert!(time.delta > 0.0, "update must receive the fixed timestep");
    // `is_running` turns 0 as soon as a quit is requested, and the remaining fixed
    // steps of that frame still run, so only record it before the threshold.
    if probe.quit_after == 0 || probe.renders < probe.quit_after {
        probe.saw_engine_running |= giochi_engine_is_running() == 1;
    }

    if probe.quit_after > 0 && probe.renders >= probe.quit_after {
        giochi_engine_request_quit();
    }
}

extern "C" fn render(user_data: *mut c_void) {
    let probe = unsafe { &mut *(user_data as *mut Probe) };
    probe.renders += 1;

    assert_eq!(
        giochi_render_clear(DRAW_COLOR),
        GIOCHI_ERROR_OK,
        "{}",
        message()
    );
    let rect = giochi_engine::Rect::new(8.0, 8.0, 32.0, 32.0);
    assert_eq!(
        giochi_render_draw_rect(&rect, DRAW_COLOR),
        GIOCHI_ERROR_OK,
        "{}",
        message()
    );
}

/// A colour the callbacks draw with.
const DRAW_COLOR: giochi_engine::Color = giochi_engine::Color::rgb(40, 160, 90);

/// The ABI layouts the header promises. Sizes are asserted here rather than in
/// `giochi-engine/tests/abi_layout.rs` because the two boundary-only structs do not
/// exist anywhere else.
#[test]
fn abi_type_layouts_match_the_contract() {
    use std::mem::{align_of, size_of};

    assert_eq!(size_of::<GiochiEngineConfig>(), 112);
    assert_eq!(align_of::<GiochiEngineConfig>(), 4);
    assert_eq!(
        size_of::<GiochiEngineConfig>(),
        size_of::<giochi_engine::EngineConfig>(),
        "the ABI config must stay layout-compatible with the engine's"
    );

    assert_eq!(size_of::<GiochiSceneCallbacks>(), 56);
    assert_eq!(align_of::<GiochiSceneCallbacks>(), 8);

    // Part III (ABI v3). Also asserted in `abi_types.rs` and, from the compiled header,
    // by `_Static_assert` in `c_smoke.c` -- a C program allocates this struct itself.
    assert_eq!(size_of::<GiochiFrameStats>(), 120);
    assert_eq!(align_of::<GiochiFrameStats>(), 8);
    // A projection, not a re-export: deliberately narrower than the Rust type.
    assert!(size_of::<GiochiFrameStats>() <= size_of::<giochi_engine::FrameStats>());

    // The error codes are part of the ABI and are renumbered only with a version bump.
    assert_eq!(GIOCHI_ERROR_OK, 0);
    assert_eq!(GIOCHI_ERROR_INVALID_HANDLE, -1);
    assert_eq!(GIOCHI_ERROR_INVALID_ARGUMENT, -2);
    assert_eq!(GIOCHI_ERROR_PLATFORM, -3);
    assert_eq!(GIOCHI_ERROR_RENDER, -4);
    assert_eq!(GIOCHI_ERROR_ASSET, -5);
    assert_eq!(GIOCHI_ERROR_AUDIO, -6);
    assert_eq!(GIOCHI_ERROR_SCENE, -7);
    assert_eq!(GIOCHI_ERROR_NOT_INITIALIZED, -8);
    assert_eq!(GIOCHI_ERROR_UNSUPPORTED, -9);
    assert_eq!(GIOCHI_ERROR_INTERNAL, -99);
}

#[test]
fn c_abi_end_to_end() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");

    // ---- version and diagnostics ------------------------------------------------
    // Part I was written against ABI 1 and must keep working now that part II (3D) has
    // shipped; the rule is "at least", never equality (docs/C_ABI_CONTRACT.md §7).
    assert!(giochi_abi_version() >= 1);
    assert_eq!(giochi_abi_version(), GIOCHI_ABI_VERSION);
    assert_eq!(
        giochi_engine_version() >> 8,
        0x0002,
        "the 0.2.x line packs to major 0, minor 2"
    );
    assert_eq!(message(), "");

    // ---- before init ------------------------------------------------------------
    assert_eq!(giochi_engine_run(), GIOCHI_ERROR_NOT_INITIALIZED);
    assert!(message().contains("not initialised"), "{}", message());
    assert_eq!(giochi_engine_scene_depth(), 0);

    // ---- init -------------------------------------------------------------------
    let title = b"capi smoke\0";
    let mut config = GiochiEngineConfig {
        width: 320,
        height: 240,
        target_fps: 60,
        vsync: 0,
        title: [0; 64],
        _reserved: [0; 8],
    };
    for (slot, byte) in config.title.iter_mut().zip(title.iter()) {
        *slot = *byte as std::os::raw::c_char;
    }

    assert_eq!(
        giochi_engine_init(&config),
        GIOCHI_ERROR_OK,
        "{}",
        message()
    );
    assert_eq!(message(), "");
    assert!((giochi_engine_timestep() - 1.0 / 60.0).abs() < 1e-6);
    assert!(!unsafe { CStr::from_ptr(giochi_engine_video_driver()) }
        .to_string_lossy()
        .is_empty());

    // A second init is refused rather than leaking the first engine.
    assert_eq!(giochi_engine_init(&config), GIOCHI_ERROR_INVALID_ARGUMENT);

    // ---- render target ----------------------------------------------------------
    let (mut width, mut height) = (0u32, 0u32);
    assert_eq!(giochi_render_size(&mut width, &mut height), GIOCHI_ERROR_OK);
    assert_eq!((width, height), (320, 240));

    // NULL out-parameters are tolerated.
    assert_eq!(
        giochi_render_size(std::ptr::null_mut(), std::ptr::null_mut()),
        GIOCHI_ERROR_OK
    );

    // ---- textures ---------------------------------------------------------------
    let player = CString::new(asset("player.png")).unwrap();
    let texture = giochi_texture_create(player.as_ptr());
    assert_ne!(texture, 0, "{}", message());
    assert_eq!(giochi_texture_width(texture), 32);
    assert_eq!(giochi_texture_height(texture), 32);

    // Loading the same path twice goes through the cache and reuses the handle.
    assert_eq!(giochi_texture_create(player.as_ptr()), texture);

    // From memory: the same PNG read through the asset loader.
    let mut size = 0usize;
    assert_eq!(
        giochi_asset_size(player.as_ptr(), &mut size),
        GIOCHI_ERROR_OK
    );
    assert!(size > 0);
    let mut bytes = vec![0u8; size];
    let mut read = 0usize;
    assert_eq!(
        giochi_asset_read(player.as_ptr(), bytes.as_mut_ptr(), bytes.len(), &mut read),
        GIOCHI_ERROR_OK,
        "{}",
        message()
    );
    assert_eq!(read, size);
    let from_memory = giochi_texture_create_memory(bytes.as_ptr(), bytes.len());
    assert_ne!(from_memory, 0, "{}", message());
    assert_eq!(giochi_texture_width(from_memory), 32);
    assert_eq!(giochi_texture_destroy(from_memory), GIOCHI_ERROR_OK);

    // Error paths: unknown handle, double destroy, bad path.
    assert_eq!(giochi_texture_destroy(999_999), GIOCHI_ERROR_INVALID_HANDLE);
    assert!(message().contains("not live"), "{}", message());
    assert_eq!(giochi_texture_width(0), 0);
    let missing = CString::new(asset("does-not-exist.png")).unwrap();
    assert_eq!(giochi_texture_create(missing.as_ptr()), 0, "{}", message());
    assert!(!message().is_empty());
    assert_eq!(
        giochi_render_draw_texture(999_999, &giochi_engine::Rect::ZERO, std::ptr::null()),
        GIOCHI_ERROR_INVALID_HANDLE
    );

    // ---- scenes -----------------------------------------------------------------
    let mut probe = Probe {
        quit_after: 4,
        ..Probe::default()
    };
    let callbacks = GiochiSceneCallbacks {
        user_data: &mut probe as *mut Probe as *mut c_void,
        on_enter: Some(on_enter),
        on_exit: Some(on_exit),
        update: Some(update),
        render: Some(render),
        _reserved: [0; 4],
    };

    // A table without `update`/`render` is rejected.
    let incomplete = GiochiSceneCallbacks {
        update: None,
        ..callbacks
    };
    assert_eq!(giochi_scene_create(&incomplete), 0);
    assert!(message().contains("update and render"), "{}", message());

    let scene = giochi_scene_create(&callbacks);
    assert_ne!(scene, 0, "{}", message());

    assert_eq!(
        giochi_engine_run_scene(scene),
        GIOCHI_ERROR_OK,
        "{}",
        message()
    );
    assert_eq!(probe.entered, 1, "on_enter must run exactly once");
    assert_eq!(probe.exited, 1, "on_exit must run exactly once");
    assert!(
        probe.saw_texture_ok,
        "on_enter must be able to create textures"
    );
    assert!(probe.renders >= 4, "the scene must have rendered");
    assert!(probe.updates > 0, "the scene must have been updated");
    assert!(
        probe.saw_engine_running,
        "is_running must be 1 inside update"
    );

    // The stack is empty afterwards, and the loop is not left claiming to run.
    assert_eq!(giochi_engine_scene_depth(), 0);
    assert_eq!(giochi_engine_is_running(), 0);
    assert!(giochi_engine_frames() >= 4);

    // The same handle can run again — that is what makes a menu loop possible.
    assert_eq!(
        giochi_engine_run_scene(scene),
        GIOCHI_ERROR_OK,
        "{}",
        message()
    );
    assert_eq!(probe.entered, 2);
    assert_eq!(probe.exited, 2);

    // ---- stepping ---------------------------------------------------------------
    let before = giochi_engine_frames();
    assert_eq!(
        giochi_engine_step(1.0 / 60.0),
        GIOCHI_ERROR_OK,
        "{}",
        message()
    );
    assert_eq!(giochi_engine_frames(), before + 1);
    // A nonsense frame time is replaced, not rejected.
    assert_eq!(giochi_engine_step(f32::NAN), GIOCHI_ERROR_OK);

    // ---- part III: frame statistics ---------------------------------------------
    let mut stats = GiochiFrameStats::default();
    assert_eq!(
        giochi_engine_frame_stats(std::ptr::null_mut()),
        GIOCHI_ERROR_INVALID_ARGUMENT
    );
    assert!(message().contains("out_stats"));
    assert_eq!(
        giochi_engine_frame_stats(&mut stats),
        GIOCHI_ERROR_OK,
        "{}",
        message()
    );
    // Reserved words are zeroed by the library, and with the `profiling` feature off
    // everything is zero -- which is a state, not a failure.
    assert_eq!(stats._reserved, [0u32; 8]);
    if cfg!(feature = "profiling") {
        assert!(stats.frames > 0, "a stepped frame must be measured");
        assert!(stats.frame_time_ns > 0);
    } else {
        assert_eq!(stats, GiochiFrameStats::default());
    }

    // ---- teardown ---------------------------------------------------------------
    assert_eq!(
        giochi_scene_destroy(scene),
        GIOCHI_ERROR_OK,
        "{}",
        message()
    );
    assert_eq!(giochi_scene_destroy(scene), GIOCHI_ERROR_INVALID_HANDLE);
    assert_eq!(giochi_texture_destroy(texture), GIOCHI_ERROR_OK);

    giochi_engine_shutdown();
    // Idempotent.
    giochi_engine_shutdown();
    assert_eq!(giochi_engine_run(), GIOCHI_ERROR_NOT_INITIALIZED);
    assert_eq!(giochi_engine_scene_depth(), 0);
}
