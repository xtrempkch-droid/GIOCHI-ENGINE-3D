//! A whole field, racing: four cars on the grid, the same rules, three laps.
//!
//! Phase 43 accepts "the opponents race the player". One car completing a lap (which
//! `tests/waypoints.rs` proves) is not that: four cars on a 10 m road have to *share* it.
//! So this file builds the game's own [`Field`] — the same struct `RaceGame` drives — puts
//! four cars on the line and checks the things that only go wrong with traffic: that
//! everybody finishes, that the quicker car finishes ahead, and that the classification is
//! a classification.
//!
//! # The player is a [`Pilot`]
//!
//! The player's car takes a [`CarInput`] from wherever the human's gamepad is; here it takes
//! one from the same controller the opponents use. That is deliberate. A player who cannot
//! steer would make this a test of three cars on an empty road, which
//! `tests/waypoints.rs` already covers, and a scripted player is the only opponent the game
//! can guarantee to exist.
//!
//! # What this test caught
//!
//! The first version of the field stepped the physics world once per car, so four cars ran
//! the world at four times the frame rate: they left the grid, crossed the barriers at about
//! 100 km/h and were in the scenery within five seconds. [`Field`] now steps the world once,
//! after everybody has proposed. Nothing about the code looked wrong; the cars' reported
//! speed was a quarter of the distance they covered, and that number is in this test's
//! failure output.

use giochi_racer::ai::Pilot;
use giochi_racer::game::physics_world;
use giochi_racer::race::Field;
use giochi_racer::track::loading::load_track;
use giochi_racer::vehicle::car::CarConfig;

const TRACKS: [(&str, &str); 3] = [
    ("assets/tracks/track_easy.scene", "OVAL EASY"),
    ("assets/tracks/track_medium.scene", "FIGURE EIGHT"),
    ("assets/tracks/track_hard.scene", "SERPENTINE HARD"),
];

const LAPS: u32 = 3;
const STEP: f32 = 1.0 / 60.0;
/// No race on the three shipped circuits takes longer than this: the slowest finish measured
/// on the serpentine is about 240 s for three laps behind traffic. A field that needs more
/// is a field that is stuck, and the failure should say so in seconds rather than hang.
const BUDGET_SECONDS: f32 = 420.0;

fn path(relative: &str) -> String {
    format!("{}/{relative}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn four_cars_share_the_road_and_all_of_them_finish() {
    for (file, name) in TRACKS {
        let track = load_track(path(file)).unwrap_or_else(|error| panic!("{file}: {error}"));
        let world = physics_world(&track).expect("a physics world");
        let mut field =
            Field::new(&track, LAPS, CarConfig::default(), world).expect("a field on the grid");

        assert_eq!(field.field_size(), 4, "{name}: the grid is not full");
        assert!(!field.opponents().is_empty(), "{name}: no opponents");

        // The player drives the line with the same controller as the opponents, from pole
        // and with no lane of its own.
        let mut player = Pilot::new(giochi_racer::ai::Route::from_track(&track), 1.0, 0.0);

        let mut used = 0;
        for frame in 0..(BUDGET_SECONDS / STEP) as u32 {
            let player_input = player.drive(&field.player_state(), STEP);
            field.step(player_input, STEP);
            if field.player_lap().is_finished()
                && field
                    .opponents()
                    .iter()
                    .all(|opponent| opponent.lap().is_finished())
            {
                used = frame + 1;
                break;
            }
        }

        for opponent in field.opponents() {
            assert!(
                opponent.lap().is_finished(),
                "{name}: {} did not finish {LAPS} laps (lap {}, gate {} of {})",
                opponent.name(),
                opponent.lap().current_lap(),
                opponent.lap().gates_taken(),
                opponent.lap().checkpoint_count() + 1
            );
        }
        assert!(
            field.player_lap().is_finished(),
            "{name}: the scripted player did not finish (lap {}, gate {} of {})",
            field.player_lap().current_lap(),
            field.player_lap().gates_taken(),
            field.player_lap().checkpoint_count() + 1
        );

        // A classification is a classification: every car once, places 1..=4, and the order
        // agrees with the positions it reports.
        let standings = field.standings();
        assert_eq!(standings.len(), 4, "{name}: {standings:?}");
        let mut names: Vec<&str> = standings
            .iter()
            .map(|standing| standing.name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec!["Azul", "Verde", "Vermelho", giochi_racer::race::PLAYER_NAME],
            "{name}"
        );
        for (index, standing) in standings.iter().enumerate() {
            assert_eq!(standing.position, index + 1, "{name}");
            assert!(
                standing.finished,
                "{name}: {} is not finished",
                standing.name
            );
        }

        // `skill` has to mean something. The 86 % car must not beat the 100 % car: if it
        // does, the multiplier is not doing what the roster says it does.
        let quickest = standings
            .iter()
            .position(|standing| standing.name == "Vermelho")
            .expect("the quickest car is classified");
        let slowest = standings
            .iter()
            .position(|standing| standing.name == "Verde")
            .expect("the slowest car is classified");
        assert!(
            quickest < slowest,
            "{name}: the 86 % car finished ahead of the 100 % car: {standings:?}"
        );

        // A car that finished by crawling is not a car that raced. Everybody's best lap
        // has to be within a factor of two of the quickest, which catches a car that spent
        // the race reversing out of a barrier without failing a slower but honest drive.
        let best_laps: Vec<(f32, f32)> = field
            .opponents()
            .iter()
            .filter_map(|opponent| {
                opponent
                    .lap()
                    .best_lap()
                    .map(|best| (opponent.skill(), best))
            })
            .collect();
        assert_eq!(best_laps.len(), 3, "{name}: an opponent has no timed lap");
        let quickest_lap = best_laps
            .iter()
            .map(|(_, best)| *best)
            .fold(f32::INFINITY, f32::min);
        for (skill, best) in &best_laps {
            assert!(
                *best < quickest_lap * 2.0,
                "{name}: the {:.0}% car's best lap was {best:.1}s against {quickest_lap:.1}s",
                skill * 100.0
            );
        }
        // The quickest drive's best lap is a lap, not a mis-counted start.
        assert!(
            quickest_lap > 5.0,
            "{name}: the quickest lap was {quickest_lap:.1}s"
        );

        eprintln!(
            "{name}: {:.1} s for {LAPS} laps — {}",
            used as f32 * STEP,
            standings
                .iter()
                .map(|standing| format!(
                    "P{} {} {:?}s",
                    standing.position,
                    standing.name,
                    if standing.name == giochi_racer::race::PLAYER_NAME {
                        field
                            .player_lap()
                            .best_lap()
                            .map(|best| format!("{best:.1}"))
                            .unwrap_or_else(|| "-".to_owned())
                    } else {
                        field
                            .opponents()
                            .iter()
                            .find(|opponent| opponent.name() == standing.name)
                            .and_then(|opponent| opponent.lap().best_lap())
                            .map(|best| format!("{best:.1}"))
                            .unwrap_or_else(|| "-".to_owned())
                    }
                ))
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }
}

#[test]
fn a_circuit_without_a_racing_line_is_still_a_time_trial() {
    // The editor can make a track before it makes a line. That must not be an error, and it
    // must not put a car on the grid with nowhere to go.
    let mut track = load_track(path(TRACKS[0].0)).expect("the oval loads");
    track.waypoints.clear();
    let world = physics_world(&track).expect("a physics world");
    let mut field = Field::new(&track, 1, CarConfig::default(), world).expect("a solo field");

    assert_eq!(field.field_size(), 1);
    assert!(field.opponents().is_empty());
    assert_eq!(field.player_position(), 1);
    assert_eq!(field.gap_ahead(), None);

    for _ in 0..60 {
        field.step(giochi_racer::vehicle::input::CarInput::NEUTRAL, STEP);
    }
    assert_eq!(field.player_position(), 1);
}

#[test]
fn a_restart_puts_the_whole_field_back_on_the_grid() {
    let track = load_track(path(TRACKS[1].0)).expect("the figure eight loads");
    let world = physics_world(&track).expect("a physics world");
    let mut field = Field::new(&track, LAPS, CarConfig::default(), world).expect("a field");

    let grid_start = field.player_state().position;
    for _ in 0..300 {
        field.step(
            giochi_racer::vehicle::input::CarInput {
                throttle: 1.0,
                steer: 0.0,
                handbrake: false,
                pause: false,
            },
            STEP,
        );
    }
    assert!(
        (field.player_state().position.x - grid_start.x).abs() > 1.0,
        "the car never left the grid, so the restart below proves nothing"
    );

    field.restart();
    assert_eq!(field.player_state().position, grid_start);
    assert_eq!(field.player_lap().current_lap(), 1);
    assert_eq!(field.player_lap().total_time(), 0.0);
    assert_eq!(field.player_position(), 1, "pole is not first on the grid");
    for opponent in field.opponents() {
        assert_eq!(opponent.lap().current_lap(), 1, "{}", opponent.name());
        assert_eq!(opponent.lap().total_time(), 0.0, "{}", opponent.name());
        assert!(opponent.state().speed_kmh < 5.0, "{}", opponent.name());
    }
}
