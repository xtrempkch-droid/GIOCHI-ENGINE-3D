//! Where the balance game's camera and its indicator markers go.
//!
//! This lives in the library rather than in `examples/ball_balance.rs` for one reason:
//! whether the markers are **on screen** is a fact that can be checked, and it was not.
//!
//! Both indicators — the raw-input cube and the hitch cube — were placed by eye against a
//! 16:9 desktop window. On the console's 640×480 panel, which is 4:3 and therefore
//! narrower, the hitch cube fell outside the horizontal field of view entirely and the
//! input cube did too as soon as the stick went to an extreme. The game looked fine on a
//! developer machine and shipped without the two things it exists to show. Nothing
//! failed, because a marker outside the frustum is not an error.
//!
//! With the layout here, `every_marker_stays_on_screen` projects the markers through this
//! camera at both aspect ratios, across the whole stick range, and fails if any of them
//! leaves the frame.

use crate::math3d::{Camera3D, Vec3};

/// The camera, the board's markers, and how the markers follow the stick.
///
/// All positions are in world units, on the same scale as
/// [`BalanceConfig`](super::BalanceConfig): the board is metres across and the camera
/// sits a few metres above and in front of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BalanceView {
    /// Where the camera sits: above and in front of the board, looking down at it.
    pub eye: Vec3,
    /// Vertical field of view, in radians. Wide enough to see the whole board.
    pub fov_y: f32,
    /// The raw-input marker's resting position.
    pub input_marker: Vec3,
    /// The hitch indicator's position.
    pub hitch_marker: Vec3,
    /// How far the raw-input marker travels per unit of stick deflection, in X and Z.
    pub marker_travel: (f32, f32),
}

impl BalanceView {
    /// The layout for a board whose half-extent is `half_board`.
    ///
    /// The markers sit **in front of** the board's near edge, so they never occlude the
    /// ball, and above it, so they do not z-fight with the platform. Their distance from
    /// the centre is a fraction of the board's own size rather than a constant, so a
    /// caller that changes the board size does not have to re-place them.
    pub fn new(half_board: f32) -> Self {
        // 1.2 board half-widths in front of the centre: outside the platform, close enough
        // to stay large on a small panel.
        let ahead = half_board * 1.2;
        let side = half_board * 0.52;
        let height = half_board * 0.43;

        Self {
            eye: Vec3::new(0.0, 6.6, 7.2),
            fov_y: 0.85,
            input_marker: Vec3::new(side, height, ahead),
            hitch_marker: Vec3::new(-side, height, ahead),
            // A quarter of a board half-width: enough to read as movement without letting
            // the marker cross the other one or leave the frame.
            marker_travel: (half_board * 0.25, half_board * 0.19),
        }
    }

    /// The camera looking down at the board.
    ///
    /// `aspect` is left at `0.0`, meaning "ask the drawable": on the console the panel's
    /// real size is authoritative, and it differs from whatever was requested (ADR-035).
    pub fn camera(&self) -> Camera3D {
        Camera3D::from_parts(self.eye, Vec3::ZERO, Vec3::UP).with_fov_y(self.fov_y)
    }

    /// Where the raw-input marker stands for a given stick deflection.
    ///
    /// No smoothing and no physics: this is the "if the marker lags your thumb, the delay
    /// is not the simulation" reference.
    pub fn input_marker_position(&self, stick: (f32, f32)) -> Vec3 {
        let (travel_x, travel_z) = self.marker_travel;
        Vec3::new(
            self.input_marker.x + stick.0 * travel_x,
            self.input_marker.y,
            self.input_marker.z + stick.1 * travel_z,
        )
    }

    /// Where the hitch indicator stands. It never moves: it is a lamp, not a cursor.
    pub fn hitch_marker_position(&self) -> Vec3 {
        self.hitch_marker
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The board the example uses: `BalanceConfig::default().platform_size / 2`.
    fn default_half_board() -> f32 {
        super::super::BalanceConfig::default().platform_size / 2.0
    }

    /// Projects `point` through `camera` at `(width, height)` and returns normalised device
    /// coordinates.
    ///
    /// `None` when the point is behind the eye. `Mat4::project_point` does the divide and
    /// refuses that case; `Mat4::transform_point` would silently hand back clip
    /// coordinates, which look like ordinary numbers and are not.
    fn project(camera: &Camera3D, point: Vec3, width: u32, height: u32) -> Option<(f32, f32)> {
        let resolved = camera.with_drawable_size(width, height);
        let ndc = resolved.view_projection().ok()?.project_point(point)?;
        Some((ndc.x, ndc.y))
    }

    #[test]
    fn every_marker_stays_on_screen() {
        let view = BalanceView::new(default_half_board());

        // The two shapes that broke it: a 720p desktop window (16:9) and the console's
        // panel (4:3). The second is the narrower one and the reason this test exists.
        for (width, height) in [(1280u32, 720u32), (640, 480)] {
            let camera = view.camera();

            let mut points = vec![("hitch", view.hitch_marker_position())];
            for stick_x in [-1.0_f32, -0.5, 0.0, 0.5, 1.0] {
                for stick_y in [-1.0_f32, 0.0, 1.0] {
                    points.push(("input", view.input_marker_position((stick_x, stick_y))));
                }
            }

            for (name, point) in points {
                let (ndc_x, ndc_y) = project(&camera, point, width, height).unwrap_or_else(|| {
                    panic!(
                        "the {name} marker at {point:?} is behind the camera on {width}x{height}"
                    )
                });

                // 0.85 rather than 1.0: a marker whose *centre* is at the very edge is
                // half off the screen, and the cube has volume. The margin is what makes
                // "visible" mean visible.
                assert!(
                    ndc_x.abs() < 0.85 && ndc_y.abs() < 0.85,
                    "the {name} marker at {point:?} lands at ndc ({ndc_x:.2}, {ndc_y:.2}) on \
                     {width}x{height}; |ndc| >= 0.85 means it is at or past the edge of the frame"
                );
            }
        }
    }

    #[test]
    fn the_markers_are_in_front_of_the_board_and_do_not_overlap() {
        let half_board = default_half_board();
        let view = BalanceView::new(half_board);

        // In front of the near edge, so they cannot hide the ball.
        assert!(
            view.hitch_marker_position().z > half_board,
            "the markers must sit beyond the platform's near edge"
        );
        // Above the platform, or they z-fight with it.
        assert!(view.hitch_marker_position().y > 0.2);
        // Apart, and not mirror-image positions that could swap when the stick moves.
        let left = view.hitch_marker_position().x;
        let right = view.input_marker.x;
        assert!(left < 0.0 && right > 0.0);

        // Even at full deflection the marker must not reach the other one.
        let furthest_left = view.input_marker_position((-1.0, 0.0)).x;
        assert!(
            furthest_left > 0.0,
            "the input marker crossed the centre line at full left deflection: {furthest_left}"
        );
    }

    #[test]
    fn the_layout_scales_with_the_board() {
        // A caller that doubles the board must get markers twice as far out, or they end up
        // on top of the ball.
        let small = BalanceView::new(3.25);
        let large = BalanceView::new(6.5);

        assert!((large.hitch_marker.z - small.hitch_marker.z * 2.0).abs() < 1e-4);
        assert!((large.hitch_marker.x - small.hitch_marker.x * 2.0).abs() < 1e-4);
        // The camera does not move: it frames the board, and the board is what scales.
        assert_eq!(large.eye, small.eye);
    }

    #[test]
    fn the_marker_follows_the_stick_in_the_expected_directions() {
        let view = BalanceView::new(3.25);

        let centre = view.input_marker_position((0.0, 0.0));
        let right = view.input_marker_position((1.0, 0.0));
        let near = view.input_marker_position((0.0, 1.0));

        // Pushing the stick right moves the marker right, and pushing it near moves it
        // towards the camera. Both were verified by hand on the console; a sign error here
        // would make the marker disagree with the platform and blame the wrong layer.
        assert!(right.x > centre.x);
        assert!((right.z - centre.z).abs() < 1e-6);
        assert!(near.z > centre.z);
        assert!((near.x - centre.x).abs() < 1e-6);
    }
}
