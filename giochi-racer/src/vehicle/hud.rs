//! The HUD: speed, gear, lap and the clock.
//!
//! Drawn with the engine's `hud` painter (`giochi_engine_3d::ui::GlesHud`), which is
//! egui rasterised over GLES 2.0 — the only text path a KMSDRM console has. The HUD is
//! drawn **after** the scene and at native resolution, so the phase-45 dynamic resolution
//! scaler cannot blur it.
//!
//! Layout follows the plan: speed and gear bottom-right where the thumb is not, lap and
//! clock top-left, and the best lap underneath them once there is one.

use giochi_engine_3d::ui::GlesHud;
use giochi_engine_3d::Result3D;

use crate::track::LapEvent;

/// Everything the HUD shows.
#[derive(Debug, Clone, PartialEq)]
pub struct HudState {
    /// Track name, shown small at the top.
    pub track: String,
    /// Ground speed in km/h.
    pub speed_kmh: f32,
    /// Simulated gear, `0` for reverse.
    pub gear: u8,
    /// Lap being driven, `1`-based.
    pub lap: u32,
    /// Laps in the race.
    pub total_laps: u32,
    /// Seconds into the current lap.
    pub lap_time: f32,
    /// Seconds since the race started.
    pub total_time: f32,
    /// The fastest lap so far.
    pub best_lap: Option<f32>,
    /// The last thing that happened to the lap, shown for a moment.
    pub event: Option<LapEvent>,
    /// Whether the race is over.
    pub finished: bool,
    /// Whether this race beat the best lap already on record for the track.
    ///
    /// Only meaningful when [`HudState::finished`]; a "NEW BEST" banner halfway through a
    /// lap would be a lie until the lap is counted.
    pub new_best: bool,
    /// The player's place on the road, `1`-based, and how many cars are in the race.
    ///
    /// Both are `0` in a solo time trial, which is what a circuit with no waypoints gets:
    /// there is nobody to be ahead of, and "P1/1" would be noise.
    pub position: usize,
    /// How many cars are racing, the player included. `0` when the player is alone.
    pub field_size: usize,
    /// Metres between the player and the car in front, if the player is not leading.
    ///
    /// A distance, not a time: the two are not the same thing on a circuit with corners,
    /// and inventing a clock the game does not keep would be a lie on the HUD.
    pub gap_ahead: Option<f32>,
}

impl HudState {
    /// The lap as `2/3`.
    pub fn lap_label(&self) -> String {
        format!("{}/{}", self.lap.min(self.total_laps), self.total_laps)
    }

    /// The gear as `3`, or `R`.
    pub fn gear_label(&self) -> String {
        if self.gear == 0 {
            "R".to_owned()
        } else {
            self.gear.to_string()
        }
    }

    /// The place as `P2/4`, or `None` in a solo run.
    pub fn position_label(&self) -> Option<String> {
        if self.field_size < 2 || self.position == 0 {
            None
        } else {
            Some(format!("P{}/{}", self.position, self.field_size))
        }
    }

    /// The gap ahead as `12 m`, or `None` when the player is leading.
    pub fn gap_label(&self) -> Option<String> {
        self.gap_ahead
            .map(|metres| format!("{:.0} m ahead", metres.max(0.0)))
    }
}

/// `m:ss.mmm`, the format a lap time is read in.
pub fn format_time(seconds: f32) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0:00.000".to_owned();
    }
    let total_ms = (seconds * 1000.0).round() as u64;
    let minutes = total_ms / 60_000;
    let secs = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    format!("{minutes}:{secs:02}.{millis:03}")
}

/// Draws the HUD and returns how many draw calls it took.
///
/// `render` supplies the GL context the painter draws on. It is passed rather than taken
/// from `hud` because [`GlesHud`] does not hand its context out — the caller owns it, and
/// keeping one owner is what stops a second context from being created by accident.
///
/// # Why the text is painted, not laid out
///
/// Every readout goes through [`egui::Painter::text`] at an absolute position. A HUD is a
/// fixed set of numbers in fixed corners; the layout engine exists to make *content*
/// flow, and putting a flow layout in a corner is how a block ends up measured against
/// the whole screen and drawn outside it. Painting at an anchored position has no
/// measuring step to get wrong, and it is one draw call.
pub fn draw(
    hud: &mut GlesHud,
    render: &giochi_engine_3d::RenderContext3D,
    screen: (u32, u32),
    delta_seconds: f32,
    state: &HudState,
) -> Result3D<usize> {
    let track = state.track.clone();
    let speed = state.speed_kmh.round() as i32;
    let gear = state.gear_label();
    let lap = state.lap_label();
    let lap_time = format_time(state.lap_time);
    let total_time = format_time(state.total_time);
    let best = state.best_lap.map(format_time);
    let finished = state.finished;
    let banner = event_message(state.event);
    let position = state.position_label();
    let gap = state.gap_label();
    let (width, height) = (screen.0 as f32, screen.1 as f32);

    hud.draw(render.glow(), screen, delta_seconds, &mut |ctx| {
        egui::Area::new("race-hud".into())
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                // A paint-only area has no widgets, so its measured size is zero and the
                // Ui's clip rectangle is that empty box: the text is submitted and nothing
                // reaches the screen. An explicit painter over the whole screen removes
                // the measurement from the equation, which is the only thing here that can
                // be wrong without an error anywhere.
                ui.set_min_size(egui::vec2(width, height));
                let screen_rect =
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
                let painter = ui.painter_at(screen_rect);
                let white = egui::Color32::WHITE;
                let grey = egui::Color32::from_gray(205);
                let dim = egui::Color32::from_gray(155);
                let mono = |size: f32| egui::FontId::monospace(size);
                let proportional = |size: f32| egui::FontId::proportional(size);

                // ---- top-left: lap and clock ------------------------------
                painter.text(
                    egui::pos2(18.0, 14.0),
                    egui::Align2::LEFT_TOP,
                    format!("LAP {lap}"),
                    proportional(28.0),
                    white,
                );
                painter.text(
                    egui::pos2(18.0, 48.0),
                    egui::Align2::LEFT_TOP,
                    &lap_time,
                    mono(28.0),
                    white,
                );
                painter.text(
                    egui::pos2(18.0, 84.0),
                    egui::Align2::LEFT_TOP,
                    format!("race {total_time}"),
                    mono(14.0),
                    grey,
                );
                if let Some(best) = &best {
                    painter.text(
                        egui::pos2(18.0, 102.0),
                        egui::Align2::LEFT_TOP,
                        format!("best {best}"),
                        mono(14.0),
                        egui::Color32::from_rgb(120, 230, 140),
                    );
                }
                painter.text(
                    egui::pos2(18.0, 120.0),
                    egui::Align2::LEFT_TOP,
                    &track,
                    proportional(12.0),
                    dim,
                );

                // ---- top-right: the place on the road ---------------------
                if let Some(position) = &position {
                    painter.text(
                        egui::pos2(width - 18.0, 14.0),
                        egui::Align2::RIGHT_TOP,
                        position,
                        proportional(28.0),
                        white,
                    );
                    if let Some(gap) = &gap {
                        painter.text(
                            egui::pos2(width - 18.0, 50.0),
                            egui::Align2::RIGHT_TOP,
                            gap,
                            mono(14.0),
                            grey,
                        );
                    }
                }

                // ---- bottom-right: speed and gear -------------------------
                painter.text(
                    egui::pos2(width - 18.0, height - 118.0),
                    egui::Align2::RIGHT_BOTTOM,
                    format!("GEAR {gear}"),
                    mono(18.0),
                    grey,
                );
                painter.text(
                    egui::pos2(width - 18.0, height - 30.0),
                    egui::Align2::RIGHT_BOTTOM,
                    format!("{speed}"),
                    mono(54.0),
                    white,
                );
                painter.text(
                    egui::pos2(width - 18.0, height - 14.0),
                    egui::Align2::RIGHT_BOTTOM,
                    "km/h",
                    proportional(14.0),
                    grey,
                );

                // ---- centre: the last lap event ---------------------------
                if let Some(banner) = &banner {
                    painter.text(
                        egui::pos2(width * 0.5, 120.0),
                        egui::Align2::CENTER_CENTER,
                        banner,
                        proportional(34.0),
                        egui::Color32::from_rgb(255, 225, 120),
                    );
                }

                // ---- the end of the race ---------------------------------
                if finished {
                    painter.text(
                        egui::pos2(width * 0.5, height * 0.42),
                        egui::Align2::CENTER_CENTER,
                        "RACE COMPLETE",
                        proportional(54.0),
                        white,
                    );
                    painter.text(
                        egui::pos2(width * 0.5, height * 0.52),
                        egui::Align2::CENTER_CENTER,
                        format!("total {total_time}"),
                        mono(24.0),
                        grey,
                    );
                    // Where the race was finished, when there was a field to finish it in.
                    if let Some(position) = &position {
                        painter.text(
                            egui::pos2(width * 0.5, height * 0.58),
                            egui::Align2::CENTER_CENTER,
                            position,
                            proportional(28.0),
                            white,
                        );
                    }
                    if let Some(best) = &best {
                        painter.text(
                            egui::pos2(width * 0.5, height * 0.60),
                            egui::Align2::CENTER_CENTER,
                            format!("best lap {best}"),
                            mono(22.0),
                            egui::Color32::from_rgb(120, 230, 140),
                        );
                    }
                    if state.new_best {
                        painter.text(
                            egui::pos2(width * 0.5, height * 0.64),
                            egui::Align2::CENTER_CENTER,
                            "NEW BEST",
                            proportional(26.0),
                            egui::Color32::from_rgb(255, 225, 120),
                        );
                    }
                    // Phase 44 turns this into buttons; until then the keys that exist
                    // are named, so a player is not stuck on a screen with no way out.
                    painter.text(
                        egui::pos2(width * 0.5, height * 0.72),
                        egui::Align2::CENTER_CENTER,
                        "R  race again        START / ESC  quit",
                        mono(16.0),
                        dim,
                    );
                }
            });
    })
}

/// The message a lap event puts on screen, if any.
pub fn event_message(event: Option<LapEvent>) -> Option<String> {
    match event? {
        LapEvent::Checkpoint(_) | LapEvent::None => None,
        LapEvent::LapComplete { lap, time, best } => Some(if best {
            format!("LAP {lap} — {time:.2}s — BEST")
        } else {
            format!("LAP {lap} — {time:.2}s")
        }),
        LapEvent::LapInvalidated => Some("LAP INVALID".to_owned()),
        LapEvent::RaceFinished => Some("FINISH".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_formats_as_minutes_seconds_and_milliseconds() {
        assert_eq!(format_time(0.0), "0:00.000");
        assert_eq!(format_time(1.0), "0:01.000");
        assert_eq!(format_time(61.5), "1:01.500");
        assert_eq!(format_time(600.0), "10:00.000");
        assert_eq!(
            format_time(59.9999),
            "1:00.000",
            "rounding to the nearest ms"
        );
    }

    #[test]
    fn a_negative_or_infinite_time_does_not_panic() {
        assert_eq!(format_time(-1.0), "0:00.000");
        assert_eq!(format_time(f32::NAN), "0:00.000");
        assert_eq!(format_time(f32::INFINITY), "0:00.000");
    }

    fn state() -> HudState {
        HudState {
            track: "oval".to_owned(),
            speed_kmh: 87.4,
            gear: 4,
            lap: 2,
            total_laps: 3,
            lap_time: 12.5,
            total_time: 40.0,
            best_lap: None,
            event: None,
            finished: false,
            new_best: false,
            position: 0,
            field_size: 0,
            gap_ahead: None,
        }
    }

    #[test]
    fn a_solo_run_shows_no_place() {
        assert_eq!(state().position_label(), None);
        assert_eq!(state().gap_label(), None);
    }

    #[test]
    fn a_race_shows_the_place_and_the_gap() {
        let mut state = state();
        state.position = 2;
        state.field_size = 4;
        state.gap_ahead = Some(12.4);
        assert_eq!(state.position_label().as_deref(), Some("P2/4"));
        assert_eq!(state.gap_label().as_deref(), Some("12 m ahead"));
    }

    #[test]
    fn the_leader_shows_a_place_but_no_gap() {
        let mut state = state();
        state.position = 1;
        state.field_size = 4;
        assert_eq!(state.position_label().as_deref(), Some("P1/4"));
        assert_eq!(state.gap_label(), None);
    }

    #[test]
    fn the_lap_label_reads_as_a_fraction() {
        assert_eq!(state().lap_label(), "2/3");
    }

    #[test]
    fn a_finished_race_does_not_show_a_fourth_lap_of_three() {
        let mut state = state();
        state.lap = 4;
        assert_eq!(state.lap_label(), "3/3");
    }

    #[test]
    fn reverse_is_shown_as_r_and_a_forward_gear_as_a_number() {
        let mut state = state();
        assert_eq!(state.gear_label(), "4");
        state.gear = 0;
        assert_eq!(state.gear_label(), "R");
    }

    #[test]
    fn the_event_messages_say_what_happened() {
        assert_eq!(event_message(None), None);
        assert_eq!(event_message(Some(LapEvent::Checkpoint(2))), None);
        assert_eq!(
            event_message(Some(LapEvent::LapComplete {
                lap: 2,
                time: 31.25,
                best: false
            })),
            Some("LAP 2 — 31.25s".to_owned())
        );
        assert_eq!(
            event_message(Some(LapEvent::LapComplete {
                lap: 1,
                time: 30.0,
                best: true
            })),
            Some("LAP 1 — 30.00s — BEST".to_owned())
        );
        assert_eq!(
            event_message(Some(LapEvent::LapInvalidated)),
            Some("LAP INVALID".to_owned())
        );
        assert_eq!(
            event_message(Some(LapEvent::RaceFinished)),
            Some("FINISH".to_owned())
        );
    }
}
