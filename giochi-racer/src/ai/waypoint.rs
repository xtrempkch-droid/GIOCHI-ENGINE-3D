//! Where to aim, given the racing line and where the car is.
//!
//! Pure geometry: no GL, no physics world, no [`Car`](crate::vehicle::car::Car). That is
//! what makes the interesting half of the AI checkable with a ruler instead of a
//! screenshot, and it is why this module has no feature flag and no setup.
//!
//! # Pure pursuit, in one paragraph
//!
//! A car cannot be told "turn 90 degrees": it can be told how hard to turn, and it turns at a
//! rate. So the controller does not aim at the next waypoint — it aims at a point a fixed
//! distance ahead along the line, and steers towards that. A short look-ahead hugs the line
//! and oscillates; a long one cuts corners. [`Route::DEFAULT_LOOK_AHEAD`] is 10 m, measured
//! on the three shipped circuits by `tests/lap_logic.rs`.
//!
//! # The line is a loop
//!
//! A lap is closed, so the waypoints are treated as a ring: the segment after the last
//! waypoint is the one from the last back to the first. The author does **not** repeat the
//! first waypoint at the end — doing so would create a zero-length segment, and a car
//! sitting on it can never finish it.

use giochi_engine_3d::math3d::Vec3;

use crate::track::Waypoint;

/// Where to aim next, and at what speed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aim {
    /// A point on the line, one look-ahead distance along it from the car.
    pub point: Vec3,
    /// The speed the line asks for where the driver is heading, in metres per second.
    pub speed: f32,
    /// The direction the line runs at the aim point, as a unit vector on the ground.
    ///
    /// A driver who wants its own lane offsets the aim point perpendicular to this, not
    /// perpendicular to its own heading: the latter is just a steering bias, which pushes
    /// the car off the road on a straight instead of moving it to one side of it.
    pub heading: Vec3,
    /// The waypoint being approached, for a gap readout or a debug overlay.
    pub waypoint: usize,
}

/// A racing line: waypoints around a lap, plus how far the driver looks ahead.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    waypoints: Vec<Waypoint>,
    look_ahead: f32,
    /// How hard the planner assumes the car can slow down, in m/s².
    brake: f32,
    /// Index of the segment being driven.
    cursor: usize,
}

impl Route {
    /// How far ahead the aim point sits, in metres.
    ///
    /// Measured, not chosen: at 5 m the car oscillates on the straights, and at 20 m it cuts
    /// the inside of the serpentine's U-turns into the barrier. 10 m is the value both the
    /// lap test and the opponents use.
    pub const DEFAULT_LOOK_AHEAD: f32 = 10.0;

    /// The smallest look-ahead that is still a controller rather than a jitter generator.
    pub const MIN_LOOK_AHEAD: f32 = 0.5;

    /// How hard the planner assumes the car can slow down, in m/s².
    ///
    /// The car itself brakes at 28, but a plan built on 28 arrives at the corner with
    /// nothing in hand and the driver spends the lap catching a slide. 10 leaves the margin
    /// that the shipped circuits were measured to need.
    pub const DEFAULT_BRAKE: f32 = 10.0;

    /// How far down the line the planner looks for a corner to slow down for, in metres.
    ///
    /// 80 m covers the longest straight on the three shipped circuits at full speed: from
    /// 105 km/h to a 27 km/h corner is a shade over 40 m at [`Route::DEFAULT_BRAKE`].
    pub const SCAN_METRES: f32 = 80.0;

    /// A line through `waypoints`, starting at the first one.
    ///
    /// An empty line is valid and aims nowhere; [`Route::is_empty`] says so, and the race
    /// then spawns no opponent rather than one that sits on the grid.
    pub fn new(waypoints: Vec<Waypoint>) -> Self {
        Self {
            waypoints,
            look_ahead: Self::DEFAULT_LOOK_AHEAD,
            brake: Self::DEFAULT_BRAKE,
            cursor: 0,
        }
    }

    /// The line read out of a loaded [`Track`](crate::track::Track).
    pub fn from_track(track: &crate::track::Track) -> Self {
        Self::new(track.waypoints.clone())
    }

    /// The same line with a different look-ahead distance, in metres.
    ///
    /// Out-of-range values are ignored rather than clamped, so a bad tuning value cannot
    /// silently become a bad controller.
    pub fn with_look_ahead(mut self, metres: f32) -> Self {
        if metres.is_finite() && metres >= Self::MIN_LOOK_AHEAD {
            self.look_ahead = metres;
        }
        self
    }

    /// The same line with a different planning deceleration, in m/s².
    pub fn with_braking(mut self, metres_per_second_squared: f32) -> Self {
        if metres_per_second_squared.is_finite() && metres_per_second_squared >= 1.0 {
            self.brake = metres_per_second_squared;
        }
        self
    }

    /// Whether there is a line to follow.
    pub fn is_empty(&self) -> bool {
        self.waypoints.is_empty()
    }

    /// How many waypoints the lap has.
    pub fn len(&self) -> usize {
        self.waypoints.len()
    }

    /// The waypoints, in lap order.
    pub fn waypoints(&self) -> &[Waypoint] {
        &self.waypoints
    }

    /// The index of the segment being driven, for a debug overlay.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Puts the line back at the start, for a restart or a rescue.
    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    /// Which waypoint the car is heading for, for a gap readout.
    pub fn next_waypoint(&self) -> Option<usize> {
        match self.waypoints.len() {
            0 => None,
            count => Some((self.cursor + 1) % count),
        }
    }

    /// The point to steer at, one look-ahead distance along the line from `position`.
    ///
    /// Advances `cursor` past segments the car has finished, wrapping at the end of the lap,
    /// so a car that overshoots a corner does not turn round and drive back to it. Returns
    /// `None` for an empty line.
    pub fn look_ahead(&mut self, position: Vec3) -> Option<Aim> {
        let count = self.waypoints.len();
        if count == 0 {
            return None;
        }
        if count == 1 {
            let only = self.waypoints[0];
            return Some(Aim {
                point: only.position,
                speed: only.speed,
                heading: Vec3::new(0.0, 0.0, 1.0),
                waypoint: 0,
            });
        }

        let car = position.to_glam();
        // Advance past segments already finished. The loop is bounded by the number of
        // waypoints: a car standing exactly on a waypoint would otherwise satisfy the
        // "passed it" test for every segment and spin here for ever.
        for _ in 0..count {
            let from = self.point(self.cursor).to_glam();
            let to = self.point(self.cursor + 1).to_glam();
            let along = to - from;
            let squared = along.length_squared();
            if squared < 1e-9 {
                // A repeated waypoint is a zero-length segment; step over it.
                self.advance();
                continue;
            }
            let t = ((car - from).dot(along) / squared).clamp(0.0, 1.0);
            if t >= 0.999 || (car - to).length() < self.look_ahead * 0.5 {
                self.advance();
            } else {
                break;
            }
        }

        let end = self.cursor + 1;
        let from = self.point(self.cursor).to_glam();
        let to = self.point(end).to_glam();
        let along = to - from;
        let squared = along.length_squared();
        if squared < 1e-9 {
            // Degenerate segment: aim at the point itself. The advance loop above normally
            // steps over these, so this only lasts a frame.
            let heading = self.point(end + 1).to_glam() - self.point(end).to_glam();
            return Some(Aim {
                point: self.point(end),
                speed: self.planned_speed(end, 0.0),
                heading: Vec3::from_glam(if heading.length_squared() > 1e-9 {
                    heading.normalize()
                } else {
                    glam::Vec3::Z
                }),
                waypoint: end % count,
            });
        }
        let t = ((car - from).dot(along) / squared).clamp(0.0, 1.0);
        let on_segment = from + along * t;
        let remaining = (to - on_segment).length();

        let direction = along.normalize();
        let (point, heading) = if remaining >= self.look_ahead {
            (on_segment + direction * self.look_ahead, direction)
        } else {
            // Look into the outgoing segment, so the car starts turning before the corner.
            let next = self.point(end + 1).to_glam();
            let after = next - to;
            if after.length_squared() < 1e-9 {
                (to, direction)
            } else {
                let outgoing = after.normalize();
                let left = (self.look_ahead - remaining).min(after.length());
                (to + outgoing * left, outgoing)
            }
        };

        Some(Aim {
            point: Vec3::from_glam(point),
            speed: self.planned_speed(end, remaining),
            heading: Vec3::from_glam(heading),
            waypoint: end % count,
        })
    }

    /// The speed the driver should be doing **now**, given the corners ahead.
    ///
    /// `end` is the waypoint being approached and `remaining` how far it still is. The
    /// answer is the fastest speed from which every corner within [`Route::SCAN_METRES`] can
    /// still be slowed down to its own speed in time — `v = sqrt(v_corner² + 2·a·s)`, the
    /// smallest over all of them.
    ///
    /// Without this the driver brakes only for the corner it is already at, which is one
    /// waypoint away, and 30 m/s does not become 8 m/s in 3 m. Measured: it was the
    /// difference between a car that completed a lap and a car that understeered into the
    /// barrier at the first corner of the oval.
    fn planned_speed(&self, end: usize, remaining: f32) -> f32 {
        let count = self.waypoints.len();
        let mut limit = self.point_speed(end);
        if count < 2 {
            return limit;
        }
        let mut travelled = remaining;
        for step in 1..count {
            let here = self.point(end + step - 1).to_glam();
            let next = self.point(end + step).to_glam();
            travelled += (next - here).length();
            if travelled > Self::SCAN_METRES {
                break;
            }
            let corner = self.point_speed(end + step);
            if corner >= limit {
                continue; // nothing to slow for
            }
            let allowed = (corner * corner + 2.0 * self.brake * travelled).sqrt();
            if allowed < limit {
                limit = allowed;
            }
        }
        limit
    }

    /// Waypoint `index`, wrapping at the end of the lap.
    fn point(&self, index: usize) -> Vec3 {
        self.waypoints[index % self.waypoints.len()].position
    }

    /// The target speed of waypoint `index`, wrapping at the end of the lap.
    fn point_speed(&self, index: usize) -> f32 {
        self.waypoints[index % self.waypoints.len()].speed
    }

    /// Moves to the next segment, wrapping at the end of the lap.
    fn advance(&mut self) {
        self.cursor = (self.cursor + 1) % self.waypoints.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waypoint(index: usize, x: f32, z: f32, speed: f32) -> Waypoint {
        Waypoint {
            index,
            position: Vec3::new(x, 0.5, z),
            speed,
        }
    }

    /// A 100 m × 100 m square, driven anticlockwise from the south-west corner, at 20 m/s.
    fn square() -> Route {
        Route::new(vec![
            waypoint(0, 0.0, 0.0, 20.0),
            waypoint(1, 100.0, 0.0, 20.0),
            waypoint(2, 100.0, 100.0, 20.0),
            waypoint(3, 0.0, 100.0, 20.0),
        ])
    }

    #[test]
    fn an_empty_line_aims_nowhere() {
        let mut route = Route::new(Vec::new());
        assert!(route.is_empty());
        assert_eq!(route.look_ahead(Vec3::ZERO), None);
        assert_eq!(route.next_waypoint(), None);
    }

    #[test]
    fn a_single_waypoint_is_the_aim_point() {
        let mut route = Route::new(vec![waypoint(0, 5.0, 5.0, 12.0)]);
        let aim = route.look_ahead(Vec3::ZERO).expect("an aim point");
        assert_eq!(aim.point, Vec3::new(5.0, 0.5, 5.0));
        assert_eq!(aim.speed, 12.0);
    }

    #[test]
    fn the_aim_point_is_the_look_ahead_distance_along_the_line() {
        let mut route = square();
        let aim = route
            .look_ahead(Vec3::new(0.0, 0.5, 0.0))
            .expect("an aim point");
        assert!((aim.point.to_glam().x - Route::DEFAULT_LOOK_AHEAD).abs() < 1e-3);
        assert!(aim.point.to_glam().z.abs() < 1e-3);
    }

    #[test]
    fn the_aim_point_stays_ahead_as_the_car_moves_along_a_straight() {
        let mut route = square();
        for x in [0.0, 10.0, 40.0, 80.0] {
            let aim = route
                .look_ahead(Vec3::new(x, 0.5, 0.0))
                .expect("an aim point");
            assert!(
                aim.point.to_glam().x >= x,
                "from x={x} the aim was behind at {:?}",
                aim.point
            );
            assert!(
                aim.point.to_glam().x - x <= Route::DEFAULT_LOOK_AHEAD + 1e-3,
                "from x={x} the aim jumped to {:?}",
                aim.point
            );
        }
    }

    #[test]
    fn a_corner_is_anticipated_before_it_is_reached() {
        let mut route = Route::new(vec![
            waypoint(0, 0.0, 0.0, 20.0),
            waypoint(1, 50.0, 0.0, 8.0),
            waypoint(2, 50.0, 50.0, 20.0),
            waypoint(3, 0.0, 50.0, 20.0),
        ]);
        // 45 m along the first leg: 5 m from the corner, so the aim is into the next leg.
        let aim = route
            .look_ahead(Vec3::new(45.0, 0.5, 0.0))
            .expect("an aim point");
        assert!(
            aim.point.to_glam().z > 0.1,
            "the aim did not turn the corner: {:?}",
            aim.point
        );
        assert_eq!(aim.speed, 8.0, "the corner speed was not the one asked for");
    }

    #[test]
    fn the_speed_asked_for_belongs_to_the_point_being_approached() {
        // A driver slows for the corner he can see, not for the straight he is on.
        let mut route = Route::new(vec![
            waypoint(0, 0.0, 0.0, 25.0),
            waypoint(1, 60.0, 0.0, 25.0),
            waypoint(2, 60.0, 60.0, 7.0),
            waypoint(3, 0.0, 60.0, 25.0),
        ]);
        let on_the_straight = route.look_ahead(Vec3::new(10.0, 0.5, 0.0)).expect("an aim");
        assert_eq!(on_the_straight.speed, 25.0);
        let into_the_corner = route.look_ahead(Vec3::new(58.0, 0.5, 0.0)).expect("an aim");
        assert_eq!(into_the_corner.speed, 7.0);
    }

    #[test]
    fn a_full_lap_wraps_the_line_back_to_the_first_waypoint() {
        let mut route = square();
        for (x, z) in [
            (10.0, 0.0),
            (95.0, 0.0),
            (100.0, 95.0),
            (5.0, 100.0),
            (0.0, 5.0),
        ] {
            let aim = route
                .look_ahead(Vec3::new(x, 0.5, z))
                .expect("an aim point");
            let gap = aim.point.to_glam().distance(glam::Vec3::new(x, 0.5, z));
            assert!(
                gap <= Route::DEFAULT_LOOK_AHEAD + 0.8,
                "from ({x}, {z}) the aim was {gap:.1} m away at {:?}",
                aim.point
            );
        }
        // Five points later the car is on the last leg, heading back to the start.
        assert_eq!(route.cursor(), 3, "the cursor did not walk the lap");
        // Round the last corner and the line wraps to the first segment.
        let aim = route
            .look_ahead(Vec3::new(0.0, 0.5, 0.0))
            .expect("an aim point");
        assert_eq!(route.cursor(), 0, "the cursor did not wrap");
        assert!(aim.point.to_glam().x > 0.0, "{:?}", aim.point);
    }

    #[test]
    fn the_cursor_walks_round_the_lap_exactly_once() {
        let mut route = square();
        let mut seen = Vec::new();
        for (x, z) in [
            (50.0, 0.0),
            (100.0, 50.0),
            (50.0, 100.0),
            (0.0, 50.0),
            (50.0, 0.0),
        ] {
            route.look_ahead(Vec3::new(x, 0.5, z));
            seen.push(route.cursor());
        }
        assert_eq!(seen, vec![0, 1, 2, 3, 0], "the cursor went {seen:?}");
    }

    #[test]
    fn a_repeated_waypoint_does_not_trap_the_controller() {
        let mut route = Route::new(vec![
            waypoint(0, 0.0, 0.0, 20.0),
            waypoint(1, 0.0, 0.0, 20.0),
            waypoint(2, 100.0, 0.0, 20.0),
        ]);
        let aim = route.look_ahead(Vec3::ZERO).expect("an aim point");
        assert!(aim.point.to_glam().is_finite(), "{:?}", aim.point);
    }

    #[test]
    fn a_look_ahead_outside_the_range_is_refused() {
        let mut too_short = square().with_look_ahead(0.1);
        let aim = too_short.look_ahead(Vec3::ZERO).expect("an aim point");
        assert!((aim.point.to_glam().x - Route::DEFAULT_LOOK_AHEAD).abs() < 1e-3);

        let mut longer = square().with_look_ahead(25.0);
        let aim = longer.look_ahead(Vec3::ZERO).expect("an aim point");
        assert!((aim.point.to_glam().x - 25.0).abs() < 1e-3);
    }

    #[test]
    fn resetting_puts_the_line_back_at_the_start() {
        let mut route = square();
        route.look_ahead(Vec3::new(90.0, 0.5, 0.0));
        route.reset();
        assert_eq!(route.cursor(), 0);
    }

    #[test]
    fn the_next_waypoint_is_the_one_after_the_cursor() {
        let mut route = square();
        assert_eq!(route.next_waypoint(), Some(1));
        // Halfway along the first leg the car is still heading for waypoint 1.
        route.look_ahead(Vec3::new(50.0, 0.5, 0.0));
        assert_eq!(route.next_waypoint(), Some(1));
        // Round the corner it is heading for the next one.
        route.look_ahead(Vec3::new(99.0, 0.5, 0.0));
        assert_eq!(route.next_waypoint(), Some(2));
    }
}
