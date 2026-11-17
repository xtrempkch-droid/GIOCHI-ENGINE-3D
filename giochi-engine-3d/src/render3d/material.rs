//! A surface: colour, emissive, metalness and roughness.
//!
//! # PBR-lite, and why "lite" is not an apology
//!
//! The renderer's light is a single directional Blinn-Phong term. That is what GLES 2.0 buys
//! on a Mali-450 and what the whole 3D path is built around (ADR-010). A full metallic-
//! roughness BRDF needs an environment probe — image-based lighting — to look like anything
//! at all; without one, a "physically based" surface is a dark grey blob and the work is
//! invisible.
//!
//! What *is* visible, and is therefore here: **emissive** (a tail light reads as lit instead
//! of as bright paint), **metalness** (a metal tints its highlight with its own colour and
//! loses its diffuse term — the difference between chrome and white plastic), and
//! **roughness** (which converts to the Phong exponent, so the number an artist types is the
//! number glTF stores).
//!
//! # Two numbers for one thing
//!
//! `roughness` and `shininess` describe the same highlight. The scene format keeps both:
//! roughness is what glTF, Blender and an artist speak; shininess is what the shader takes.
//! [`Material::shininess_from_roughness`] is the single conversion, and
//! [`Material::resolved_shininess`] is what the renderer uploads.
//!
//! # C-shaped
//!
//! `#[repr(C)]` with primitive fields and trailing reserved space, like
//! [`DirectionalLight`](crate::DirectionalLight), so it can cross the C ABI later without a
//! redesign.

use crate::math3d::Color3D;

/// A surface as the renderer shades it.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    /// Linear RGBA base colour.
    pub base_color: Color3D,
    /// Linear RGB emitted regardless of lighting. Black is off.
    pub emissive: Color3D,
    /// Specular strength, `0.0..=1.0`.
    pub specular: f32,
    /// Metalness, `0.0..=1.0`. `1.0` tints the highlight with `base_color` and removes the
    /// diffuse term.
    pub metallic: f32,
    /// Roughness, `0.0..=1.0`. `0.0` is a mirror, `1.0` is fully diffuse.
    pub roughness: f32,
    /// The Phong exponent, used when [`Material::roughness`] is left at
    /// [`Material::ROUGHNESS_FROM_SHININESS`] (the "not set" marker).
    pub shininess: f32,
    /// Whether back faces are drawn.
    pub double_sided: bool,
    /// Reserved for ABI-compatible growth. Must be zero.
    pub _reserved: [u32; 3],
}

impl Material {
    /// The roughness at which the exponent is taken from [`Material::shininess`] instead.
    ///
    /// Not a physical value — it is the scene format's default, used as the marker for "this
    /// material never set a roughness". A file that wants roughness 0.5 exactly can still get
    /// a different highlight through `shininess`, which is the behaviour the older scenes
    /// were written against.
    pub const ROUGHNESS_FROM_SHININESS: f32 = 0.5;

    /// The exponent a perfectly smooth surface gets. Above 128 the highlight is smaller than
    /// a pixel on the resolutions this targets.
    pub const MAX_SHININESS: f32 = 128.0;

    /// A plain white matte surface.
    pub const DEFAULT: Self = Self {
        base_color: Color3D::WHITE,
        emissive: Color3D::BLACK,
        specular: 0.25,
        metallic: 0.0,
        roughness: Self::ROUGHNESS_FROM_SHININESS,
        shininess: 32.0,
        double_sided: false,
        _reserved: [0; 3],
    };

    /// The Phong exponent this material's roughness asks for.
    ///
    /// The standard conversion (`2 / roughness² − 2`, clamped): a mirror is 128, a rough
    /// surface bottoms out at 1, which `pow` handles without dividing by zero.
    pub fn shininess_from_roughness(&self) -> f32 {
        let roughness = self.roughness.clamp(0.01, 1.0);
        (2.0 / (roughness * roughness) - 2.0).clamp(1.0, Self::MAX_SHININESS)
    }

    /// The exponent to upload: derived from roughness unless roughness was never set.
    pub fn resolved_shininess(&self) -> f32 {
        if (self.roughness - Self::ROUGHNESS_FROM_SHININESS).abs() <= f32::EPSILON {
            self.shininess.clamp(1.0, Self::MAX_SHININESS)
        } else {
            self.shininess_from_roughness()
        }
    }

    /// Whether anything is emitted at all, for a caller that can skip a uniform.
    pub fn emits(&self) -> bool {
        self.emissive.r > 0.0 || self.emissive.g > 0.0 || self.emissive.b > 0.0
    }

    /// The same material with a different base colour.
    pub const fn with_base_color(mut self, base_color: Color3D) -> Self {
        self.base_color = base_color;
        self
    }

    /// The same material with an emissive colour.
    pub const fn with_emissive(mut self, emissive: Color3D) -> Self {
        self.emissive = emissive;
        self
    }

    /// The same material with a roughness, which takes over from `shininess`.
    pub const fn with_roughness(mut self, roughness: f32) -> Self {
        self.roughness = roughness;
        self
    }

    /// The same material with a metalness.
    pub const fn with_metallic(mut self, metallic: f32) -> Self {
        self.metallic = metallic;
        self
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mirror_gets_the_steepest_exponent() {
        let material = Material::DEFAULT.with_roughness(0.0);
        assert_eq!(material.shininess_from_roughness(), Material::MAX_SHININESS);
    }

    #[test]
    fn a_rough_surface_gets_the_flattest_exponent() {
        let material = Material::DEFAULT.with_roughness(1.0);
        // 2/1 - 2 = 0, clamped to 1: `pow(x, 1.0)` is a broad sheen, and 0 would make the
        // highlight a constant.
        assert_eq!(material.shininess_from_roughness(), 1.0);
    }

    #[test]
    fn the_exponent_falls_monotonically_as_roughness_rises() {
        // Rougher surface, broader highlight, smaller exponent. Written as a monotonicity
        // check rather than as a table of values, so the curve can be tuned without the test
        // turning into a copy of the implementation.
        let mut previous = f32::INFINITY;
        for step in 0..=10 {
            let roughness = step as f32 / 10.0;
            let exponent = Material::DEFAULT
                .with_roughness(roughness)
                .shininess_from_roughness();
            assert!(
                exponent <= previous,
                "roughness {roughness} gave {exponent} after {previous}"
            );
            previous = exponent;
        }
    }

    #[test]
    fn the_shininess_field_wins_while_roughness_is_unset() {
        let mut material = Material::DEFAULT;
        material.shininess = 64.0;
        assert_eq!(material.resolved_shininess(), 64.0, "the old scene's value");

        material.roughness = 0.2;
        assert_eq!(
            material.resolved_shininess(),
            material.shininess_from_roughness(),
            "an explicit roughness takes over"
        );
    }

    #[test]
    fn a_shininess_of_zero_is_clamped_instead_of_dividing_by_nothing() {
        let mut material = Material::DEFAULT;
        material.shininess = 0.0;
        assert_eq!(material.resolved_shininess(), 1.0);
    }

    #[test]
    fn an_emissive_colour_is_reported_as_emitting() {
        assert!(!Material::DEFAULT.emits());
        assert!(Material::DEFAULT
            .with_emissive(Color3D::new(0.1, 0.0, 0.0, 1.0))
            .emits());
    }

    #[test]
    fn the_builders_do_not_disturb_the_other_fields() {
        let material = Material::DEFAULT
            .with_base_color(Color3D::new(0.5, 0.25, 0.125, 1.0))
            .with_roughness(0.25)
            .with_metallic(0.75);
        assert_eq!(material.base_color.r, 0.5);
        assert_eq!(material.roughness, 0.25);
        assert_eq!(material.metallic, 0.75);
        assert_eq!(material.emissive, Color3D::BLACK);
    }
}
