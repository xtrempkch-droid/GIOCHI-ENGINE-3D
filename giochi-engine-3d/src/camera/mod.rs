//! Game cameras: follow, shake and cutscenes.
//!
//! The editor has its own orbit camera (`giochi-editor`); these are the ones a *game* uses,
//! and they answer three different questions:
//!
//! - [`FollowCamera`] — "keep the player on screen, smoothly". A lag towards a point behind
//!   the target, with look-ahead so a car turning at speed shows where it is going rather
//!   than where it has been.
//! - [`ShakeCamera`] — "make the hit felt". Trauma that decays, mapped to an offset by a
//!   deterministic noise function: no `rand` dependency, and the same replay shakes the same
//!   way.
//! - [`CutsceneCamera`] — "show this, then this". Keyframes with per-segment easing.
//!
//! # Why the smoothing is frame-rate independent
//!
//! A naive `position += (target - position) * 0.1` moves twice as fast at 120 fps as at 60.
//! The convergence here is `1 - exp(-stiffness * dt)`, which is the exact solution of the
//! first-order lag over `dt` and therefore the same *over time* at any frame rate. It is also
//! always in `0..=1`, so the camera can never overshoot — the failure mode of a spring with a
//! badly chosen constant.
//!
//! # Everything is `f32` and pure math
//!
//! No GL, no window: a camera is a function of time and of what it is looking at, and keeping
//! it that way is what lets all of this be tested without a driver. [`CameraDirector`] hands
//! the result to `Camera3D` only at the end.

use crate::math3d::{Camera3D, Vec3};

/// A keyframe of a cutscene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraKey {
    /// Seconds from the start of the cutscene at which this key is reached.
    pub time: f32,
    /// Where the camera is.
    pub eye: Vec3,
    /// Where it looks.
    pub look_at: Vec3,
    /// How the move *into* this key is eased.
    pub easing: Easing,
}

impl CameraKey {
    /// A key at `time`, eased with [`Easing::SmoothStep`].
    pub const fn new(time: f32, eye: Vec3, look_at: Vec3) -> Self {
        Self {
            time,
            eye,
            look_at,
            easing: Easing::SmoothStep,
        }
    }

    /// The same key with a different easing.
    pub const fn with_easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }
}

/// How a segment between two keys interpolates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Easing {
    /// Constant speed. Right for a pan; wrong for a camera that starts and stops.
    Linear,
    /// Smooth at both ends. The default, and what most shots want.
    SmoothStep,
    /// Slow at the start, fast at the end: a push in.
    EaseIn,
    /// Fast at the start, slow at the end: a reveal.
    EaseOut,
}

impl Easing {
    /// Maps a `0..=1` progress through the curve.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            // 3t² − 2t³: zero derivative at both ends, which is what reads as "smooth".
            Self::SmoothStep => t * t * (3.0 - 2.0 * t),
            Self::EaseIn => t * t,
            Self::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

/// A camera that trails a moving target.
#[derive(Debug, Clone, PartialEq)]
pub struct FollowCamera {
    /// How far behind the target the eye sits, in world units.
    pub distance: f32,
    /// How far above it.
    pub height: f32,
    /// How quickly the eye catches up. Higher is snappier; `0` would never move.
    pub stiffness: f32,
    /// How far ahead of the target's velocity the camera looks, in seconds of travel.
    pub look_ahead: f32,
    eye: Vec3,
    look: Vec3,
    started: bool,
}

impl Default for FollowCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl FollowCamera {
    /// A camera with the defaults a racing game wants: 6 units behind, 2.5 above, and a
    /// third of a second of look-ahead.
    pub fn new() -> Self {
        Self {
            distance: 6.0,
            height: 2.5,
            stiffness: 6.0,
            look_ahead: 0.35,
            eye: Vec3::ZERO,
            look: Vec3::ZERO,
            started: false,
        }
    }

    /// Whether the camera has been placed. Before the first update there is no meaningful
    /// eye, and a caller that draws anyway would put the view at the origin.
    pub const fn started(&self) -> bool {
        self.started
    }

    /// The current eye.
    pub const fn eye(&self) -> Vec3 {
        self.eye
    }

    /// The current look-at point.
    pub const fn look_at(&self) -> Vec3 {
        self.look
    }

    /// Where the camera *wants* to be for `target` moving at `velocity`.
    ///
    /// The desired look-at leads the target along its velocity, and the eye sits behind it —
    /// so a car at speed looks further down the road and the camera swings around to follow.
    pub fn desired(&self, target: Vec3, velocity: Vec3) -> (Vec3, Vec3) {
        let look = add(target, scale(velocity, self.look_ahead));
        let eye = add(look, Vec3::new(0.0, self.height, self.distance));
        (eye, look)
    }

    /// Jumps straight to the desired place, with no smoothing.
    ///
    /// What a respawn needs: a camera that eases across the map after a teleport is a camera
    /// the player watches fly instead of playing.
    pub fn snap(&mut self, target: Vec3, velocity: Vec3) {
        let (eye, look) = self.desired(target, velocity);
        self.eye = eye;
        self.look = look;
        self.started = true;
    }

    /// Advances towards the target by `dt` seconds.
    pub fn update(&mut self, target: Vec3, velocity: Vec3, dt: f32) {
        if !self.started {
            self.snap(target, velocity);
            return;
        }
        let (eye, look) = self.desired(target, velocity);
        let blend = blend_factor(self.stiffness, dt);
        self.eye = lerp(self.eye, eye, blend);
        self.look = lerp(self.look, look, blend);
    }

    /// This camera as the renderer's camera, for a drawable of `width`×`height`.
    pub fn to_camera3d(&self, width: u32, height: u32) -> Camera3D {
        Camera3D::from_parts(self.eye, self.look, Vec3::UP)
            .with_drawable_size(width.max(1), height.max(1))
    }
}

/// A decaying shake, added on top of whatever the other cameras do.
#[derive(Debug, Clone, PartialEq)]
pub struct ShakeCamera {
    /// How much shake is left, `0.0..=1.0`.
    trauma: f32,
    /// Seconds since the shake started, driving the noise.
    time: f32,
    /// How fast trauma decays, per second.
    pub decay: f32,
    /// How far the eye moves at full trauma, in world units.
    pub amplitude: f32,
    /// How far the look-at point moves at full trauma, in world units.
    pub look_amplitude: f32,
    /// A per-session seed, so two shakes in one session are not identical.
    seed: u32,
}

impl Default for ShakeCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl ShakeCamera {
    /// A shake that is at rest.
    pub fn new() -> Self {
        Self {
            trauma: 0.0,
            time: 0.0,
            decay: 1.6,
            amplitude: 0.35,
            look_amplitude: 0.12,
            seed: 0x9e37_79b9,
        }
    }

    /// Adds trauma, clamped to `1.0`. Two small hits add up; a hundred do not blow up.
    pub fn add_trauma(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount.max(0.0)).clamp(0.0, 1.0);
        // A new hit moves the seed, so a second explosion is a second shake rather than more
        // of the same one.
        self.seed = self
            .seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
    }

    /// The current trauma, `0.0..=1.0`.
    pub const fn trauma(&self) -> f32 {
        self.trauma
    }

    /// Whether there is anything left to shake.
    pub fn is_active(&self) -> bool {
        self.trauma > 0.0
    }

    /// Decays the trauma by `dt` seconds.
    pub fn update(&mut self, dt: f32) {
        self.time += dt.max(0.0);
        self.trauma = (self.trauma - self.decay * dt.max(0.0)).max(0.0);
    }

    /// The eye offset this frame.
    ///
    /// `trauma²` rather than `trauma`: a small hit is a small shake, and the falloff reads as
    /// an impact rather than as a switch. The channels are decorrelated by different seeds
    /// and different frequencies, so the eye does not move in a straight line.
    pub fn eye_offset(&self) -> Vec3 {
        if self.trauma <= 0.0 {
            return Vec3::ZERO;
        }
        let amount = self.trauma * self.trauma * self.amplitude;
        Vec3::new(
            (noise(self.seed, self.time * 27.0) * 2.0 - 1.0) * amount,
            (noise(self.seed ^ 0x5bd1_e995, self.time * 31.0) * 2.0 - 1.0) * amount,
            (noise(self.seed ^ 0x1b87_3593, self.time * 23.0) * 2.0 - 1.0) * amount,
        )
    }

    /// The look-at offset this frame, smaller than the eye's: shaking the aim as much as the
    /// viewpoint makes a scene unreadable.
    pub fn look_offset(&self) -> Vec3 {
        if self.trauma <= 0.0 {
            return Vec3::ZERO;
        }
        let amount = self.trauma * self.trauma * self.look_amplitude;
        Vec3::new(
            (noise(self.seed ^ 0xa511_e9b3, self.time * 19.0) * 2.0 - 1.0) * amount,
            (noise(self.seed ^ 0x68e3_1da4, self.time * 29.0) * 2.0 - 1.0) * amount,
            0.0,
        )
    }
}

/// A scripted camera move.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CutsceneCamera {
    keys: Vec<CameraKey>,
}

impl CutsceneCamera {
    /// An empty cutscene. Sampling one returns `None`; a caller should not play an empty one,
    /// and [`CutsceneCamera::is_empty`] says so.
    pub fn new() -> Self {
        Self::default()
    }

    /// A cutscene from keys, which are sorted by time (a file can list them in any order).
    pub fn from_keys(mut keys: Vec<CameraKey>) -> Self {
        keys.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Self { keys }
    }

    /// Adds a key, keeping the list sorted.
    pub fn push(&mut self, key: CameraKey) {
        self.keys.push(key);
        self.keys.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// How many keys it has.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether it has no keys.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// How long the whole move takes: the time of the last key.
    pub fn duration(&self) -> f32 {
        self.keys.last().map_or(0.0, |key| key.time)
    }

    /// Whether `elapsed` is past the end.
    pub fn is_finished(&self, elapsed: f32) -> bool {
        elapsed >= self.duration()
    }

    /// The eye and look-at at `elapsed` seconds.
    ///
    /// Before the first key it holds the first; after the last it holds the last. That is
    /// what makes a cutscene safe to sample out of range — which is what a caller that runs
    /// one frame too long does.
    pub fn sample(&self, elapsed: f32) -> Option<(Vec3, Vec3)> {
        let first = self.keys.first()?;
        if elapsed <= first.time {
            return Some((first.eye, first.look_at));
        }
        let last = self.keys.last()?;
        if elapsed >= last.time {
            return Some((last.eye, last.look_at));
        }
        for pair in self.keys.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            if elapsed >= from.time && elapsed <= to.time {
                let span = (to.time - from.time).max(1e-6);
                let progress = ((elapsed - from.time) / span).clamp(0.0, 1.0);
                let eased = to.easing.apply(progress);
                return Some((
                    lerp(from.eye, to.eye, eased),
                    lerp(from.look_at, to.look_at, eased),
                ));
            }
        }
        Some((last.eye, last.look_at))
    }
}

/// The camera a game feeds to the renderer: a follow camera, a shake on top, and an optional
/// cutscene that takes over until it ends.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraDirector {
    /// The trailing camera.
    pub follow: FollowCamera,
    /// The shake, added to whatever the active camera says.
    pub shake: ShakeCamera,
    /// The cutscene playing, if any.
    cutscene: Option<CutsceneCamera>,
    /// Seconds since the cutscene started.
    elapsed: f32,
}

impl CameraDirector {
    /// A director with a default follow camera and no shake.
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts `cutscene`, replacing any that is playing.
    pub fn play(&mut self, cutscene: CutsceneCamera) {
        self.elapsed = 0.0;
        self.cutscene = Some(cutscene);
    }

    /// Stops the cutscene, returning to the follow camera immediately.
    pub fn skip(&mut self) {
        self.cutscene = None;
        self.elapsed = 0.0;
    }

    /// Whether a cutscene is playing.
    pub const fn is_playing_cutscene(&self) -> bool {
        self.cutscene.is_some()
    }

    /// Advances everything by `dt` seconds.
    ///
    /// The follow camera keeps updating during a cutscene — that is what makes the return
    /// after one seamless instead of a jump from wherever the cutscene left the view.
    pub fn update(&mut self, target: Vec3, velocity: Vec3, dt: f32) {
        self.follow.update(target, velocity, dt);
        self.shake.update(dt);
        if let Some(cutscene) = &self.cutscene {
            self.elapsed += dt.max(0.0);
            if cutscene.is_finished(self.elapsed) {
                self.cutscene = None;
                self.elapsed = 0.0;
            }
        }
    }

    /// The eye to render from.
    pub fn eye(&self) -> Vec3 {
        let (eye, _) = self.base_view();
        add(eye, self.shake.eye_offset())
    }

    /// The look-at point to render at.
    pub fn look_at(&self) -> Vec3 {
        let (_, look) = self.base_view();
        add(look, self.shake.look_offset())
    }

    /// This director as the renderer's camera.
    pub fn to_camera3d(&self, width: u32, height: u32) -> Camera3D {
        Camera3D::from_parts(self.eye(), self.look_at(), Vec3::UP)
            .with_drawable_size(width.max(1), height.max(1))
    }

    /// The eye and look-at before the shake, from the cutscene or the follow camera.
    fn base_view(&self) -> (Vec3, Vec3) {
        match &self.cutscene {
            Some(cutscene) => cutscene
                .sample(self.elapsed)
                .unwrap_or((self.follow.eye(), self.follow.look_at())),
            None => (self.follow.eye(), self.follow.look_at()),
        }
    }
}

/// `1 - exp(-stiffness * dt)`, clamped: the exact solution of a first-order lag over `dt`.
///
/// This is the whole reason the camera behaves the same at 30 and 144 fps.
pub fn blend_factor(stiffness: f32, dt: f32) -> f32 {
    if stiffness <= 0.0 || dt <= 0.0 {
        return 0.0;
    }
    (1.0 - (-stiffness * dt).exp()).clamp(0.0, 1.0)
}

/// A deterministic noise in `0.0..=1.0` for an integer seed and a real phase.
///
/// A hash, not a PRNG: no state to seed, no crate to depend on, and the same inputs give the
/// same shake on every run and every machine — which is what makes a replay reproducible.
fn noise(seed: u32, phase: f32) -> f32 {
    let mut value = seed ^ (phase.to_bits().wrapping_mul(0x9e37_79b9));
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    (value >> 8) as f32 / (1u32 << 24) as f32
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::from_glam(a.to_glam() + b.to_glam())
}

fn scale(a: Vec3, factor: f32) -> Vec3 {
    Vec3::from_glam(a.to_glam() * factor)
}

fn lerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    Vec3::from_glam(a.to_glam().lerp(b.to_glam(), t.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distance(a: Vec3, b: Vec3) -> f32 {
        (a.to_glam() - b.to_glam()).length()
    }

    #[test]
    fn the_first_update_places_the_camera_instead_of_easing_from_the_origin() {
        let mut camera = FollowCamera::new();
        assert!(!camera.started());
        camera.update(Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO, 1.0 / 60.0);
        assert!(camera.started());
        let (eye, look) = camera.desired(Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO);
        assert!(distance(camera.eye(), eye) < 1e-4, "{:?}", camera.eye());
        assert!(distance(camera.look_at(), look) < 1e-4);
    }

    #[test]
    fn the_camera_converges_on_the_target_without_overshooting() {
        let mut camera = FollowCamera::new();
        let target = Vec3::new(20.0, 0.0, 0.0);
        camera.snap(Vec3::ZERO, Vec3::ZERO);
        let (goal, _) = camera.desired(target, Vec3::ZERO);
        let mut previous = distance(camera.eye(), goal);
        for _ in 0..600 {
            camera.update(target, Vec3::ZERO, 1.0 / 60.0);
            let now = distance(camera.eye(), goal);
            assert!(
                now <= previous + 1e-4,
                "the camera overshot: {now} > {previous}"
            );
            previous = now;
        }
        assert!(previous < 0.01, "it never arrived: {previous}");
    }

    #[test]
    fn the_smoothing_is_frame_rate_independent() {
        // The same second of catch-up, in 60 and in 240 steps, has to end in the same place.
        let mut slow = FollowCamera::new();
        let mut fast = FollowCamera::new();
        let target = Vec3::new(5.0, 0.0, 0.0);
        slow.snap(Vec3::ZERO, Vec3::ZERO);
        fast.snap(Vec3::ZERO, Vec3::ZERO);
        for _ in 0..60 {
            slow.update(target, Vec3::ZERO, 1.0 / 60.0);
        }
        for _ in 0..240 {
            fast.update(target, Vec3::ZERO, 1.0 / 240.0);
        }
        assert!(
            distance(slow.eye(), fast.eye()) < 0.05,
            "60 fps {:?} vs 240 fps {:?}",
            slow.eye(),
            fast.eye()
        );
    }

    #[test]
    fn the_look_ahead_leans_into_the_velocity() {
        let camera = FollowCamera::new();
        let target = Vec3::ZERO;
        let (_, still) = camera.desired(target, Vec3::ZERO);
        let (_, moving) = camera.desired(target, Vec3::new(0.0, 0.0, -10.0));
        assert!(moving.z < still.z - 1.0, "{moving:?} vs {still:?}");
    }

    #[test]
    fn snapping_lands_exactly_on_the_desired_place() {
        let mut camera = FollowCamera::new();
        let target = Vec3::new(-4.0, 1.0, 2.0);
        camera.snap(target, Vec3::ZERO);
        let (eye, look) = camera.desired(target, Vec3::ZERO);
        assert!(distance(camera.eye(), eye) < 1e-6);
        assert!(distance(camera.look_at(), look) < 1e-6);
    }

    #[test]
    fn the_blend_factor_is_bounded_and_monotonic_in_dt() {
        assert_eq!(blend_factor(0.0, 1.0), 0.0);
        assert_eq!(blend_factor(6.0, 0.0), 0.0);
        let mut previous = 0.0;
        for step in 1..=20 {
            let blend = blend_factor(6.0, step as f32 / 60.0);
            assert!((0.0..=1.0).contains(&blend), "{blend}");
            assert!(blend > previous, "{blend} after {previous}");
            previous = blend;
        }
        assert!(blend_factor(1000.0, 1.0) <= 1.0);
    }

    #[test]
    fn every_easing_starts_at_zero_ends_at_one_and_never_goes_backwards() {
        for easing in [
            Easing::Linear,
            Easing::SmoothStep,
            Easing::EaseIn,
            Easing::EaseOut,
        ] {
            assert!(easing.apply(0.0).abs() < 1e-6, "{easing:?}");
            assert!((easing.apply(1.0) - 1.0).abs() < 1e-6, "{easing:?}");
            assert!(easing.apply(-1.0).abs() < 1e-6, "{easing:?} extrapolated");
            assert!(
                (easing.apply(2.0) - 1.0).abs() < 1e-6,
                "{easing:?} extrapolated"
            );
            let mut previous = -1.0;
            for step in 0..=10 {
                let value = easing.apply(step as f32 / 10.0);
                assert!(value >= previous, "{easing:?} went backwards at {step}");
                previous = value;
            }
        }
    }

    #[test]
    fn trauma_decays_to_zero_and_stops_shaking() {
        let mut shake = ShakeCamera::new();
        assert!(!shake.is_active());
        shake.add_trauma(0.5);
        assert!(shake.is_active());
        assert!(shake.eye_offset().length() > 0.0);
        for _ in 0..120 {
            shake.update(1.0 / 60.0);
        }
        assert!(!shake.is_active(), "trauma {} left", shake.trauma());
        assert_eq!(shake.eye_offset(), Vec3::ZERO);
        assert_eq!(shake.look_offset(), Vec3::ZERO);
    }

    #[test]
    fn trauma_is_clamped_and_two_hits_add_up() {
        let mut shake = ShakeCamera::new();
        shake.add_trauma(0.4);
        shake.add_trauma(0.4);
        assert!((shake.trauma() - 0.8).abs() < 1e-5);
        shake.add_trauma(5.0);
        assert_eq!(shake.trauma(), 1.0);
        shake.add_trauma(-1.0);
        assert_eq!(shake.trauma(), 1.0, "a negative hit is not a heal");
    }

    #[test]
    fn the_shake_offset_stays_inside_its_amplitude() {
        let mut shake = ShakeCamera::new();
        shake.add_trauma(1.0);
        for step in 0..200 {
            shake.update(1.0 / 60.0);
            let offset = shake.eye_offset();
            assert!(
                offset.length() <= shake.amplitude * 1.8,
                "step {step}: {offset:?}"
            );
            let look = shake.look_offset();
            assert!(look.length() <= shake.look_amplitude * 1.8, "step {step}");
        }
    }

    #[test]
    fn the_shake_is_deterministic() {
        let mut a = ShakeCamera::new();
        let mut b = ShakeCamera::new();
        a.add_trauma(0.7);
        b.add_trauma(0.7);
        for _ in 0..30 {
            a.update(1.0 / 60.0);
            b.update(1.0 / 60.0);
            assert_eq!(a.eye_offset(), b.eye_offset());
        }
    }

    #[test]
    fn a_cutscene_interpolates_between_its_keys() {
        let cutscene = CutsceneCamera::from_keys(vec![
            CameraKey::new(0.0, Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0)),
            CameraKey::new(2.0, Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 0.0, -1.0))
                .with_easing(Easing::Linear),
        ]);
        assert_eq!(cutscene.duration(), 2.0);
        let (eye, _) = cutscene.sample(0.0).expect("a key");
        assert!(distance(eye, Vec3::ZERO) < 1e-6);
        let (eye, _) = cutscene.sample(1.0).expect("a key");
        assert!((eye.x - 5.0).abs() < 1e-4, "{eye:?}");
        let (eye, _) = cutscene.sample(2.0).expect("a key");
        assert!((eye.x - 10.0).abs() < 1e-4);
        // Out of range clamps to the ends rather than extrapolating into space.
        let (eye, _) = cutscene.sample(-5.0).expect("a key");
        assert!(distance(eye, Vec3::ZERO) < 1e-6);
        let (eye, _) = cutscene.sample(99.0).expect("a key");
        assert!((eye.x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn keys_are_sorted_by_time_whatever_order_they_arrive_in() {
        let mut cutscene = CutsceneCamera::new();
        cutscene.push(CameraKey::new(3.0, Vec3::new(3.0, 0.0, 0.0), Vec3::ZERO));
        cutscene.push(CameraKey::new(1.0, Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO));
        cutscene.push(CameraKey::new(2.0, Vec3::new(2.0, 0.0, 0.0), Vec3::ZERO));
        assert_eq!(cutscene.len(), 3);
        assert_eq!(cutscene.duration(), 3.0);
        let (eye, _) = cutscene.sample(1.5).expect("a key");
        assert!(eye.x > 1.0 && eye.x < 2.0, "{eye:?}");
    }

    #[test]
    fn an_empty_cutscene_has_nothing_to_sample() {
        let cutscene = CutsceneCamera::new();
        assert!(cutscene.is_empty());
        assert_eq!(cutscene.duration(), 0.0);
        assert!(cutscene.sample(0.0).is_none());
        assert!(cutscene.is_finished(0.0));
    }

    #[test]
    fn a_cutscene_takes_over_and_hands_back_to_the_follow_camera() {
        let mut director = CameraDirector::new();
        director.follow.snap(Vec3::ZERO, Vec3::ZERO);
        director.play(CutsceneCamera::from_keys(vec![
            CameraKey::new(0.0, Vec3::new(0.0, 10.0, 0.0), Vec3::ZERO),
            CameraKey::new(1.0, Vec3::new(5.0, 10.0, 0.0), Vec3::ZERO).with_easing(Easing::Linear),
        ]));
        assert!(director.is_playing_cutscene());
        director.update(Vec3::ZERO, Vec3::ZERO, 0.0);
        assert!(director.eye().y > 9.0, "{:?}", director.eye());

        director.update(Vec3::ZERO, Vec3::ZERO, 0.5);
        assert!(
            (director.eye().x - 2.5).abs() < 1e-3,
            "{:?}",
            director.eye()
        );

        // Past the end: back to following, which kept updating during the cutscene.
        director.update(Vec3::ZERO, Vec3::ZERO, 0.6);
        assert!(!director.is_playing_cutscene());
        let (eye, _) = director.follow.desired(Vec3::ZERO, Vec3::ZERO);
        assert!(distance(director.eye(), eye) < 1e-3, "{:?}", director.eye());
    }

    #[test]
    fn skipping_a_cutscene_returns_to_the_follow_camera_at_once() {
        let mut director = CameraDirector::new();
        director.follow.snap(Vec3::new(1.0, 2.0, 3.0), Vec3::ZERO);
        director.play(CutsceneCamera::from_keys(vec![CameraKey::new(
            5.0,
            Vec3::new(0.0, 100.0, 0.0),
            Vec3::ZERO,
        )]));
        assert!(director.eye().y > 50.0);
        director.skip();
        assert!(!director.is_playing_cutscene());
        assert!(director.eye().y < 10.0);
    }

    #[test]
    fn the_shake_is_added_on_top_of_whatever_the_camera_is_doing() {
        let mut director = CameraDirector::new();
        director.follow.snap(Vec3::ZERO, Vec3::ZERO);
        let calm = director.eye();
        director.shake.add_trauma(1.0);
        director.update(Vec3::ZERO, Vec3::ZERO, 1.0 / 60.0);
        assert_ne!(director.eye(), calm, "the shake has to move the eye");
    }

    #[test]
    fn the_director_starts_at_a_usable_camera() {
        let director = CameraDirector::new();
        let camera = director.to_camera3d(1280, 720);
        assert!(camera.position.is_finite());
        assert!(camera.target.is_finite());
        assert!(camera.near_plane > 0.0);
        assert!(camera.far_plane > camera.near_plane);
    }
}
