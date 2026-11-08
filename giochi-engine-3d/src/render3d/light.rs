//! One directional light, shared by every draw in a frame.
//!
//! A directional light models a source so far away that its rays are parallel: the sun,
//! a moon, a floodlight over a stadium. It has a direction and no position, so nothing
//! attenuates and nothing needs to know how far a surface is from the light. That is
//! also the cheapest useful light — six numbers and no loop — which is why it is the
//! only one in GLES 2.0 territory here.
//!
//! # Why one light per frame instead of per draw
//!
//! The uniforms live on the [`RenderContext3D`](crate::RenderContext3D), not on the mesh
//! or the shader. A frame therefore has one sun, which is what every scene in this
//! repository wants and what keeps the per-draw uniform traffic to a fixed five values
//! instead of scaling with the number of objects.
//!
//! # The direction points the way the light travels
//!
//! [`DirectionalLight::sun`] points *down*, `(0, -1, 0)`: the light travels towards the
//! floor. The shader negates it once to get the vector *towards* the light, which is
//! what a dot product with a normal needs. The convention is `direction` = "the way the
//! photons go", matching how a camera has a forward and how GLSL's `reflect` thinks; if
//! it were the other way, a scene lit from above would need a direction with a positive
//! Y, and every reader would get it wrong once.
//!
//! # C-shaped
//!
//! [`repr(C)`](DirectionalLight) with primitive fields only (rule R1) and trailing
//! reserved space (rule R3). [`Vec3`] and [`Color3D`] are themselves `#[repr(C)]`, so
//! this struct can cross the C ABI when phase 14 exposes it, without a redesign.

use crate::math3d::{Color3D, Vec3};

/// The light every draw in a frame is shaded with.
///
/// The direction convention is spelled out on [`DirectionalLight::direction`] and in
/// `docs/3D.md` §5.1: the field follows the photons, not the eye.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectionalLight {
    /// The direction the light **travels**, normalised. `(0, -1, 0)` is straight down.
    pub direction: Vec3,
    /// The colour and intensity of the light. `WHITE` is full strength; values above
    /// 1.0 are legal and deliberately blow highlights out, which is how a bright sun
    /// reads.
    pub color: Color3D,
    /// How much of the surface colour survives where the light does not reach.
    ///
    /// Zero makes unlit faces black, which hides geometry; around 0.2–0.35 keeps the
    /// shape readable. Clamped to `0.0..=1.0` when used.
    pub ambient: f32,
    /// The strength of the specular highlight. Zero is a matte surface.
    pub specular: f32,
    /// The exponent of the specular highlight: 8 is a broad sheen, 128 a tight glint.
    /// Must be at least 1, or `pow` turns the highlight into a disc.
    pub shininess: f32,
    /// Reserved for ABI-compatible growth. Must be zero.
    pub _reserved: [u32; 4],
}

impl DirectionalLight {
    /// A white sun high and slightly to one side, with a matte finish.
    ///
    /// This is the default the renderer starts with: bright enough that an unlit face
    /// is still visible, and neutral enough that it does not tint a caller's colours.
    pub const fn sun() -> Self {
        Self {
            // Not exactly `(0, -1, 0)`: a light straight down leaves every vertical face
            // with the same diffuse term, and a cube then reads as a flat hexagon. Tilting
            // it makes adjacent faces differ, which is what makes the wireframe toggle
            // unnecessary for telling orientation apart.
            direction: Vec3::new(0.35, -1.0, 0.25),
            color: Color3D::WHITE,
            ambient: 0.3,
            specular: 0.25,
            shininess: 32.0,
            _reserved: [0; 4],
        }
    }

    /// A light travelling in `direction`, normalised on use.
    pub const fn with_direction(mut self, direction: Vec3) -> Self {
        self.direction = direction;
        self
    }

    /// Sets the light colour.
    pub const fn with_color(mut self, color: Color3D) -> Self {
        self.color = color;
        self
    }

    /// Sets the ambient floor.
    pub const fn with_ambient(mut self, ambient: f32) -> Self {
        self.ambient = ambient;
        self
    }

    /// Sets the specular strength and exponent together: they only make sense as a pair.
    pub const fn with_specular(mut self, strength: f32, shininess: f32) -> Self {
        self.specular = strength;
        self.shininess = shininess;
        self
    }

    /// A copy with every field a finite number in the range the shader expects.
    ///
    /// The renderer calls this before uploading, so a caller cannot produce a black
    /// frame by forgetting that `shininess` is an exponent, that a zero-length direction
    /// normalises to NaN, or by handing over a field it never initialised — which is easy
    /// to do from C, where `GiochiDirectionalLight` has no constructor and an uninitialised
    /// `float` is whatever the stack held.
    ///
    /// The three scalars need different handling because the standard library's methods
    /// differ on NaN: `f32::max` returns the *other* operand, `f32::clamp` returns `self`.
    /// So `specular.max(0.0)` and `shininess.max(1.0)` already swallow a NaN, while
    /// `ambient.clamp(0.0, 1.0)` passes it through — and `NaN` in `u_ambient_strength`
    /// makes every fragment `NaN`, which is the black frame this function exists to
    /// prevent.
    pub fn sanitised(self) -> Self {
        // A zero-length direction would normalise to NaN in the shader and every
        // fragment would come out NaN, which most drivers render as black. Falling back
        // to the sun's own direction keeps the frame lit and is visible in the log.
        let length_squared = self.direction.x * self.direction.x
            + self.direction.y * self.direction.y
            + self.direction.z * self.direction.z;
        let direction = if length_squared > 1e-12 && length_squared.is_finite() {
            self.direction.normalized()
        } else {
            log::warn!("the light direction is degenerate; falling back to straight down");
            Vec3::new(0.0, -1.0, 0.0)
        };

        Self {
            direction,
            color: self.color.clamped(),
            // Zero is the honest answer for an ambient the caller did not mean: it makes
            // unlit faces black but leaves the lit ones correct, which is readable. A
            // `NaN` would take the whole frame with it, and an infinity is already handled
            // by the `clamp`.
            ambient: if self.ambient.is_nan() {
                log::warn!("the light's ambient is NaN; using 0.0");
                0.0
            } else {
                self.ambient.clamp(0.0, 1.0)
            },
            // An infinite specular is not a number the shader can use: it becomes an
            // infinite highlight, which saturates the whole surface to white. Clamping to a
            // finite ceiling keeps the reflection but stops it eating the frame.
            specular: finite_or(self.specular, 0.0, "specular").clamp(0.0, MAX_SPECULAR),
            // `pow(x, 0)` is 1 everywhere, so a zero exponent paints a constant
            // highlight over the whole lit hemisphere instead of a glint. An infinite one
            // underflows to 0 everywhere except the exact eye direction.
            shininess: finite_or(self.shininess, 1.0, "shininess")
                .clamp(MIN_SHININESS, MAX_SHININESS),
            _reserved: [0; 4],
        }
    }
}

/// Ceiling for the specular strength. The default sun uses 0.25; a caller reaching this is
/// asking for a mirror, which is as far as a single Blinn-Phong term goes before the
/// highlight covers the whole surface.
const MAX_SPECULAR: f32 = 8.0;

/// The smallest usable specular exponent, and the largest.
///
/// Below 1 the `pow` term is 1 almost everywhere; above a few thousand the highlight is
/// smaller than a fragment at any sane resolution.
const MIN_SHININESS: f32 = 1.0;
const MAX_SHININESS: f32 = 4096.0;

/// Returns `value` when it is finite, otherwise `fallback` and a warning.
fn finite_or(value: f32, fallback: f32, field: &str) -> f32 {
    if value.is_finite() {
        value
    } else {
        log::warn!("the light's {field} is {value}; using {fallback}");
        fallback
    }
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self::sun()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};

    #[test]
    fn layout_is_c_compatible() {
        // Pinned so a field added in the middle — which would shift every later field and
        // silently corrupt a C caller — fails here instead of on a console. `Vec3` and
        // `Color3D` carry their own `_reserved` tail (rule R3), which is why they are 28
        // and 32 bytes rather than 12 and 16.
        assert_eq!(size_of::<Vec3>(), 28);
        assert_eq!(size_of::<Color3D>(), 32);
        assert_eq!(size_of::<DirectionalLight>(), 28 + 32 + 4 + 4 + 4 + 16);
        // Worst field alignment is 4 (`f32`); a `glow` or `glam` type would raise it.
        assert_eq!(align_of::<DirectionalLight>(), 4);
        assert_eq!(size_of::<DirectionalLight>() % 4, 0);
    }

    #[test]
    fn the_default_sun_points_downwards() {
        let light = DirectionalLight::sun();
        assert!(
            light.direction.y < 0.0,
            "the light must travel towards the floor, not away from it: {:?}",
            light.direction
        );
    }

    #[test]
    fn the_default_sun_is_not_axis_aligned() {
        // A light exactly straight down leaves every vertical face equally lit; the
        // default deliberately avoids that so a cube's faces are distinguishable.
        let light = DirectionalLight::sun();
        assert!(
            light.direction.x.abs() > 0.0 && light.direction.z.abs() > 0.0,
            "the default light must have an off-axis component: {:?}",
            light.direction
        );
    }

    #[test]
    fn sanitising_normalises_the_direction() {
        let light = DirectionalLight::sun()
            .with_direction(Vec3::new(0.0, -4.0, 0.0))
            .sanitised();

        assert!((light.direction.length() - 1.0).abs() < 1e-5);
        assert!((light.direction.y + 1.0).abs() < 1e-5);
    }

    #[test]
    fn sanitising_replaces_a_degenerate_direction() {
        // A NaN direction would make every fragment NaN; most drivers render that as
        // black, so the symptom is "the scene vanished" rather than a lighting bug.
        for degenerate in [Vec3::ZERO, Vec3::new(f32::NAN, 0.0, 0.0)] {
            let light = DirectionalLight::sun()
                .with_direction(degenerate)
                .sanitised();

            assert!(light.direction.is_finite(), "{degenerate:?} survived");
            assert!((light.direction.length() - 1.0).abs() < 1e-5);
            assert!(light.direction.y < 0.0, "must fall back to pointing down");
        }
    }

    #[test]
    fn sanitising_clamps_every_range() {
        let light = DirectionalLight::sun()
            .with_ambient(9.0)
            .with_specular(-2.0, 0.0)
            .with_color(Color3D::new(4.0, -1.0, 0.5, 2.0))
            .sanitised();

        assert_eq!(light.ambient, 1.0);
        assert_eq!(light.specular, 0.0);
        // `pow(x, 0)` is 1 everywhere, which paints the whole lit hemisphere with the
        // highlight instead of a glint.
        assert_eq!(light.shininess, 1.0);
        assert_eq!(light.color.to_array(), [1.0, 0.0, 0.5, 1.0]);
    }

    #[test]
    fn a_nan_ambient_does_not_survive() {
        // `f32::clamp` returns `self` when it is NaN, unlike `f32::max`, which returns the
        // other operand. `ambient` was the one field that used `clamp` directly, so a NaN
        // went through and `gl_FragColor = ambient + direct + highlight` became NaN for
        // every fragment — a black frame, produced by the function whose documented job is
        // to prevent one. Reachable from C: `GiochiDirectionalLight` has no constructor, so
        // an uninitialised `ambient` is whatever was on the stack.
        let mut light = DirectionalLight::sun();
        light.ambient = f32::NAN;

        assert_eq!(light.sanitised().ambient, 0.0);
    }

    #[test]
    fn every_sanitised_light_field_is_finite() {
        // The catch-all: whatever a caller writes into the struct, the values uploaded as
        // uniforms are real numbers in range.
        let mut light = DirectionalLight::sun();
        light.direction = Vec3::new(f32::NAN, f32::INFINITY, f32::NEG_INFINITY);
        light.color = Color3D::new(f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::NAN);
        light.ambient = f32::NAN;
        light.specular = f32::NAN;
        light.shininess = f32::NAN;

        let light = light.sanitised();

        assert!(light.direction.is_finite());
        let channels = light.color.to_array();
        assert!(channels.iter().all(|c| c.is_finite()), "{channels:?}");
        assert!(light.ambient.is_finite());
        assert!(light.specular.is_finite());
        assert!(light.shininess.is_finite());
        assert!((0.0..=1.0).contains(&light.ambient));
        assert!(light.specular >= 0.0);
        assert!(light.shininess >= 1.0);

        // And the infinity variants of the scalar fields, which `clamp` already handled
        // for `ambient` but not for the other two.
        for field in 0..3 {
            let mut light = DirectionalLight::sun();
            match field {
                0 => light.ambient = f32::INFINITY,
                1 => light.specular = f32::INFINITY,
                _ => light.shininess = f32::INFINITY,
            }
            let light = light.sanitised();
            assert!(light.ambient.is_finite() && (0.0..=1.0).contains(&light.ambient));
            assert!(light.specular.is_finite());
            assert!(light.shininess.is_finite());
        }
    }

    #[test]
    fn sanitising_is_idempotent() {
        // `sun()` stores a direction it has not normalised, so the first pass does change
        // it; what must hold is that a second pass changes nothing, because the renderer
        // sanitises on every `set_light` and a caller may pass back what it reads.
        let once = DirectionalLight::sun().sanitised();
        let twice = once.sanitised();

        assert_eq!(twice, once);

        // The same for a light that needed repair.
        let mut broken = DirectionalLight::sun();
        broken.ambient = f32::NAN;
        broken.specular = f32::INFINITY;
        broken.direction = Vec3::ZERO;
        let repaired = broken.sanitised();
        assert_eq!(repaired.sanitised(), repaired);
    }

    #[test]
    fn the_builder_methods_are_the_only_way_to_keep_reserved_zero() {
        let light = DirectionalLight::sun()
            .with_direction(Vec3::UP)
            .with_color(Color3D::BLACK)
            .with_ambient(0.0)
            .with_specular(0.5, 16.0)
            .sanitised();

        assert_eq!(light._reserved, [0; 4]);
        assert_eq!(DirectionalLight::default()._reserved, [0; 4]);
    }
}
