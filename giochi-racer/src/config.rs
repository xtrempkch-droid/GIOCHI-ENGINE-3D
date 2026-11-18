//! What a race is configured with.
//!
//! Kept apart from the car and the camera so a phase-44 settings screen can hand the whole
//! file to a slider panel without reaching into either: the tuning lives next to the code
//! that consumes it ([`crate::vehicle::car::CarConfig`], [`crate::vehicle::camera::ChaseConfig`])
//! and this type only decides which track, how many laps and which of each to use.
//!
//! # Why a path and not a `Track`
//!
//! A track cannot be loaded without a GL context (its meshes are GPU handles), and the
//! config is read before the window exists. The track is therefore a path until
//! [`crate::game::RaceGame::new`], which is the first place that has a context to load it
//! into.

use std::path::{Path, PathBuf};

use crate::vehicle::camera::ChaseConfig;
use crate::vehicle::car::CarConfig;

/// The track a bare `giochi-racer` opens.
///
/// Relative to the executable's directory first and the working directory second (see
/// [`crate::assets::resolve`]), which is what makes the phase-46 install work: the game
/// ships `assets/` next to the binary, not next to wherever it happens to be started from.
pub const DEFAULT_TRACK: &str = "assets/tracks/track_easy.scene";

/// Laps in a race. Three is the plan's number: long enough for a mistake to be recoverable,
/// short enough to finish in a couple of minutes while testing.
pub const DEFAULT_LAPS: u32 = 3;

/// Everything a race needs before the window is open.
#[derive(Debug, Clone, PartialEq)]
pub struct GameConfig {
    /// The `.scene` file to race on.
    pub track: PathBuf,
    /// How many laps finish the race.
    pub laps: u32,
    /// Car tuning.
    pub car: CarConfig,
    /// Chase-camera tuning.
    pub camera: ChaseConfig,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            track: PathBuf::from(DEFAULT_TRACK),
            laps: DEFAULT_LAPS,
            car: CarConfig::default(),
            camera: ChaseConfig::default(),
        }
    }
}

impl GameConfig {
    /// The same config pointed at `track`.
    pub fn with_track(mut self, track: impl AsRef<Path>) -> Self {
        self.track = track.as_ref().to_path_buf();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_config_races_three_laps_on_the_easy_track() {
        let config = GameConfig::default();
        assert_eq!(config.laps, DEFAULT_LAPS);
        assert_eq!(
            config.track,
            PathBuf::from("assets/tracks/track_easy.scene")
        );
        assert_eq!(config.car, CarConfig::default());
        assert_eq!(config.camera, ChaseConfig::default());
    }

    #[test]
    fn with_track_keeps_the_rest_of_the_config() {
        let config = GameConfig::default().with_track("elsewhere/hard.scene");
        assert_eq!(config.track, PathBuf::from("elsewhere/hard.scene"));
        assert_eq!(config.laps, DEFAULT_LAPS);
        assert_eq!(config.car, CarConfig::default());
    }
}
