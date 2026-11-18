//! GIOCHI RACER — an arcade racer built with the GIOCHI editor.
//!
//! Salto C of the master plan: a game that *consumes* `giochi-engine` and `giochi-editor`
//! and changes neither (R2). The library holds everything except the platform loop, so
//! the simulation can be tested without a window and phase 44's scene stack can reuse the
//! pieces:
//!
//! | Module | What it owns |
//! |---|---|
//! | [`config`] | which track, how many laps, how the car handles |
//! | [`track`] | an editor `.scene` as a circuit: meshes, colliders, gates, lap counting |
//! | [`vehicle`] | the car, its input, its camera and its HUD |
//! | [`game`] | one race: the frame, in order |
//! | [`utils`] | the procedural car and the transform composition the engine lacks |
//! | [`assets`] | where the files are, in a checkout and in an installed package |
//!
//! # Where the plan's numbers come from
//!
//! The tracks are `.scene` files the editor reads and writes — data, not Rust (R3). The
//! naming convention that turns a scene into a circuit (`Start`, `Checkpoint 1`…) is in
//! `docs/TRACK_SPECS.md`, and `tests/track_specs.rs` checks the shipped track against the
//! plan's dimensions (200 m, 10 m wide, four checkpoints) on every `cargo test`.

pub mod assets;
pub mod config;
pub mod game;
pub mod screenshot;
pub mod track;
pub mod utils;
pub mod vehicle;

/// Semantic version of the crate, as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_matches_the_workspace() {
        // Compared against the engine rather than a literal: every member inherits the
        // workspace version, and a hardcoded string here is exactly the line a release
        // forgets to touch.
        assert_eq!(VERSION, giochi_engine::VERSION);
    }
}
