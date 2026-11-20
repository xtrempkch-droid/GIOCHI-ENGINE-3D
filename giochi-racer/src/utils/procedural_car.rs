//! The low-poly car, generated instead of downloaded.
//!
//! The plan allows either a CC0 model or a procedural one (R7), and forbids inventing
//! URLs. A procedural car has three advantages the asset does not: it is a few hundred
//! bytes of arithmetic instead of a binary in the repository, it scales with
//! [`CarConfig`] so the collider and the picture cannot disagree, and its wheels can spin
//! because they are separate meshes rather than one baked model.
//!
//! The shape is the plan's: one box for the body, one for the cabin, four cylinders for
//! the wheels. That is 1 200 triangles' worth of geometry *before* the low-poly promise is
//! broken, so it stays well under the 500 the plan budgets.

use giochi_engine_3d::math3d::{Color3D, Quat, Transform3D, Vec3};
use giochi_engine_3d::mesh::Vertex;
use giochi_engine_3d::render3d::{MeshHandle, RenderContext3D};
use giochi_engine_3d::Result3D;

use crate::vehicle::car::CarConfig;

/// Which of the three generated meshes a part uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarPartShape {
    /// The body box.
    Body,
    /// The cabin box.
    Cabin,
    /// One wheel cylinder.
    Wheel,
}

/// One piece of the car, in the car's own space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarPart {
    /// Which mesh to draw.
    pub shape: CarPartShape,
    /// Where, relative to the car's centre.
    pub transform: Transform3D,
    /// What colour.
    pub color: Color3D,
}

/// Body paint.
pub const BODY_COLOR: Color3D = Color3D::new(0.86, 0.24, 0.18, 1.0);
/// Glass.
pub const CABIN_COLOR: Color3D = Color3D::new(0.14, 0.18, 0.26, 1.0);
/// Tyres.
pub const WHEEL_COLOR: Color3D = Color3D::new(0.07, 0.07, 0.08, 1.0);

/// A wheel's width as a fraction of its radius.
pub const WHEEL_WIDTH_RATIO: f32 = 0.7;

/// How many segments a wheel has. Twelve reads round at the distance a chase camera
/// keeps without costing anything.
pub const WHEEL_SEGMENTS: u32 = 12;

/// The car's parts for a given tuning and wheel rotation.
///
/// `wheel_spin` is radians about `+X`; callers advance it from the distance travelled, so
/// the wheels turn at the right rate for the speed without any animation state.
pub fn parts(config: &CarConfig, wheel_spin: f32) -> Vec<CarPart> {
    parts_with_paint(config, wheel_spin, BODY_COLOR)
}

/// The same parts, with the body painted `paint`.
///
/// The opponents are the same car as the player — same mesh, same wheels, same cab — and
/// only the paint tells them apart. Giving them a model of their own would be a second
/// thing to keep in step for no gain.
pub fn parts_with_paint(config: &CarConfig, wheel_spin: f32, paint: Color3D) -> Vec<CarPart> {
    let half = config.half_extents.to_glam();
    let door = half.x.abs() * 2.0;
    let length = half.z.abs() * 2.0;
    let height = half.y.abs() * 2.0;
    let radius = config.wheel_radius;

    let mut parts = Vec::with_capacity(6);
    parts.push(CarPart {
        shape: CarPartShape::Body,
        transform: Transform3D::new(Vec3::ZERO, Quat::IDENTITY, Vec3::new(door, height, length)),
        color: paint,
    });
    parts.push(CarPart {
        shape: CarPartShape::Cabin,
        transform: Transform3D::new(
            // Towards the back of the car and above the belt line.
            Vec3::new(0.0, height * 0.5 + height * 0.22, -length * 0.12),
            Quat::IDENTITY,
            Vec3::new(door * 0.78, height * 0.62, length * 0.42),
        ),
        color: CABIN_COLOR,
    });

    // The axle sits out by the wheel's half width, so the cylinder's inner face is
    // flush with the body rather than buried inside it.
    let axle_x = half.x + radius * WHEEL_WIDTH_RATIO * 0.5;
    let axle_z = length * 0.32;
    let axle_y = radius - half.y;
    for (x, z) in [
        (-axle_x, axle_z),
        (axle_x, axle_z),
        (-axle_x, -axle_z),
        (axle_x, -axle_z),
    ] {
        parts.push(CarPart {
            shape: CarPartShape::Wheel,
            transform: Transform3D::new(
                Vec3::new(x, axle_y, z),
                Quat::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), wheel_spin),
                Vec3::new(1.0, 1.0, 1.0),
            ),
            color: WHEEL_COLOR,
        });
    }
    parts
}

/// The three meshes a car needs, uploaded once.
#[derive(Debug, Clone, Copy)]
pub struct CarMeshes {
    /// Unit box, scaled per part.
    pub body: MeshHandle,
    /// Unit box, scaled per part.
    pub cabin: MeshHandle,
    /// A wheel cylinder, shared by all four.
    pub wheel: MeshHandle,
}

impl CarMeshes {
    /// Uploads the three meshes.
    pub fn create(render: &mut RenderContext3D, config: &CarConfig) -> Result3D<Self> {
        let body = render.create_cube_mesh(1.0)?;
        let cabin = render.create_cube_mesh(1.0)?;
        let (vertices, indices) = cylinder(
            config.wheel_radius,
            config.wheel_radius * WHEEL_WIDTH_RATIO,
            WHEEL_SEGMENTS,
        );
        let wheel = render.create_mesh(&vertices, &indices)?;
        Ok(Self { body, cabin, wheel })
    }

    /// The mesh handle for a part.
    pub fn handle(&self, shape: CarPartShape) -> MeshHandle {
        match shape {
            CarPartShape::Body => self.body,
            CarPartShape::Cabin => self.cabin,
            CarPartShape::Wheel => self.wheel,
        }
    }

    /// Frees the three meshes.
    pub fn destroy(&self, render: &mut RenderContext3D) {
        render.destroy_mesh(self.body);
        render.destroy_mesh(self.cabin);
        render.destroy_mesh(self.wheel);
    }
}

/// A closed cylinder whose axis is `+X`, centred on the origin.
///
/// Wound counter-clockwise seen from outside, which is what the renderer's back-face
/// culling expects: the side quads are `(a, c, b) (c, d, b)` and the caps fan out from
/// their centre in the direction of their own normal.
pub fn cylinder(radius: f32, width: f32, segments: u32) -> (Vec<Vertex>, Vec<u32>) {
    let radius = radius.abs().max(1e-4);
    let half = width.abs().max(1e-4) * 0.5;
    let segments = segments.max(3);
    let turn = std::f32::consts::TAU / segments as f32;

    let mut vertices =
        Vec::with_capacity((segments as usize + 1) * 2 + (segments as usize + 2) * 2);
    let mut indices = Vec::with_capacity((segments * 12) as usize);

    // ---- side ------------------------------------------------------------
    for step in 0..=segments {
        let angle = step as f32 * turn;
        let (sin, cos) = angle.sin_cos();
        let normal = [0.0, cos, sin];
        let (y, z) = (radius * cos, radius * sin);
        vertices.push(Vertex::flat([-half, y, z], normal));
        vertices.push(Vertex::flat([half, y, z], normal));
    }
    for step in 0..segments {
        let a = step * 2;
        let b = a + 1;
        let c = a + 2;
        let d = a + 3;
        indices.extend_from_slice(&[a, c, b, c, d, b]);
    }

    // ---- caps ------------------------------------------------------------
    for side in [-1.0f32, 1.0] {
        let x = half * side;
        let normal = [side, 0.0, 0.0];
        let centre = vertices.len() as u32;
        vertices.push(Vertex::flat([x, 0.0, 0.0], normal));
        let rim = vertices.len() as u32;
        for step in 0..=segments {
            let angle = step as f32 * turn;
            let (sin, cos) = angle.sin_cos();
            vertices.push(Vertex::flat([x, radius * cos, radius * sin], normal));
        }
        for step in 0..segments {
            let first = rim + step;
            let second = first + 1;
            if side > 0.0 {
                indices.extend_from_slice(&[centre, first, second]);
            } else {
                indices.extend_from_slice(&[centre, second, first]);
            }
        }
    }

    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cylinder_is_closed_and_every_index_is_in_range() {
        let (vertices, indices) = cylinder(0.3, 0.2, 12);
        assert!(!vertices.is_empty());
        assert_eq!(indices.len() % 3, 0);
        for index in &indices {
            assert!(
                (*index as usize) < vertices.len(),
                "index {index} out of range"
            );
        }
        // Two endpoints plus two caps.
        assert_eq!(vertices.len(), (12 + 1) * 2 + (12 + 2) * 2);
    }

    #[test]
    fn every_cylinder_normal_is_a_unit_vector() {
        let (vertices, _) = cylinder(0.3, 0.2, 8);
        for vertex in &vertices {
            let [x, y, z] = vertex.normal;
            let length = (x * x + y * y + z * z).sqrt();
            assert!(
                (length - 1.0).abs() < 1e-4,
                "{vertex:?} has normal length {length}"
            );
        }
    }

    #[test]
    fn the_cylinder_is_as_wide_and_as_round_as_it_was_asked_to_be() {
        let (vertices, _) = cylinder(0.5, 0.2, 16);
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        let mut radius = 0.0f32;
        for vertex in &vertices {
            let [x, y, z] = vertex.position;
            min = min.min(x);
            max = max.max(x);
            radius = radius.max((y * y + z * z).sqrt());
        }
        assert!((max - min - 0.2).abs() < 1e-5, "width {} ", max - min);
        assert!((radius - 0.5).abs() < 1e-4, "radius {radius}");
    }

    #[test]
    fn a_degenerate_request_still_produces_a_mesh() {
        let (vertices, indices) = cylinder(0.0, 0.0, 1);
        assert!(!vertices.is_empty());
        assert!(!indices.is_empty());
        for vertex in &vertices {
            assert!(vertex.position.iter().all(|value| value.is_finite()));
        }
    }

    #[test]
    fn the_car_has_a_body_a_cabin_and_four_wheels() {
        let parts = parts(&CarConfig::default(), 0.0);
        assert_eq!(parts.len(), 6);
        assert_eq!(
            parts
                .iter()
                .filter(|part| part.shape == CarPartShape::Wheel)
                .count(),
            4
        );
        assert_eq!(
            parts
                .iter()
                .filter(|part| part.shape == CarPartShape::Body)
                .count(),
            1
        );
        assert_eq!(
            parts
                .iter()
                .filter(|part| part.shape == CarPartShape::Cabin)
                .count(),
            1
        );
    }

    #[test]
    fn the_body_scales_with_the_collider() {
        let config = CarConfig::default();
        let parts = parts(&config, 0.0);
        let body = parts[0];
        assert_eq!(body.transform.scale.to_array(), [1.9, 0.7, 4.0]);
        // The wheels sit on the road when the body's centre is at `resting_height`.
        for part in parts
            .iter()
            .filter(|part| part.shape == CarPartShape::Wheel)
        {
            let world_y = config.resting_height() + part.transform.position.to_array()[1];
            assert!(
                (world_y - config.wheel_radius).abs() < 1e-5,
                "wheel centre at {world_y}, radius {}",
                config.wheel_radius
            );
        }
    }

    #[test]
    fn the_wheels_spin_about_x() {
        let spun = parts(&CarConfig::default(), 1.0);
        let still = parts(&CarConfig::default(), 0.0);
        for (spun, still) in spun.iter().zip(still.iter()) {
            if spun.shape == CarPartShape::Wheel {
                assert_ne!(spun.transform.rotation, still.transform.rotation);
            } else {
                assert_eq!(
                    spun.transform.rotation, still.transform.rotation,
                    "the body does not spin"
                );
            }
        }
    }

    #[test]
    fn a_narrow_car_keeps_its_wheels_outside_the_body() {
        let config = CarConfig {
            half_extents: Vec3::new(0.6, 0.3, 1.5),
            ..CarConfig::default()
        };
        let parts = parts(&config, 0.0);
        let body_half = parts[0].transform.scale.to_array()[0] * 0.5;
        for wheel in parts
            .iter()
            .filter(|part| part.shape == CarPartShape::Wheel)
        {
            let x = wheel.transform.position.to_array()[0];
            // The cylinder's inner edge, not its centre, is what must clear the body.
            let inner = x.abs() - config.wheel_radius * WHEEL_WIDTH_RATIO * 0.5;
            assert!(
                inner >= body_half - 1e-3,
                "wheel at {x} inside body half {body_half}"
            );
        }
    }
}
