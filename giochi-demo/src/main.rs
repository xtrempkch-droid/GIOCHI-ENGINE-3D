//! GIOCHI-ENGINE demo binary.
//!
//! Phase 0: bootstrap smoke check — links the engine, prints the version banner
//! and exits. Phase 1 replaces this with a 1280x720 black window that closes on
//! `Escape`.

use giochi_engine::{sdl_version, ABI_VERSION, VERSION};

fn main() {
    env_logger::init();

    println!("giochi-engine bootstrap OK");

    let (sdl_major, sdl_minor, sdl_patch) = sdl_version();
    log::info!("engine {VERSION}, C ABI v{ABI_VERSION}, SDL2 {sdl_major}.{sdl_minor}.{sdl_patch}");
}
