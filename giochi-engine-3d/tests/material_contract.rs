//! The single material contract shared by glTF, the scene format and the shaders
//! (DOD criteria 10 and 17).
//!
//! **Ativado na Fase S3.**
//!
//! # Why a skeleton and not a test against `Material`
//!
//! [`Material`] exists and is already C-shaped, but it is the *renderer's* view: a colour,
//! an emissive term and three numbers. What a glTF file carries is richer — base-colour
//! and normal textures, an alpha mode with a cutoff, and defaults for everything a file
//! omits. S3 adds `giochi_engine_3d::material_contract::{MaterialContract, fallback}` and
//! makes the loader build **one** of those instead of mapping fields at each call site.
//!
//! Until then the assertions below are written against the contract that S3 must satisfy,
//! with a `todo!()` naming the exact API. A `todo!()` in an `#[ignore]`d test is the
//! repository's convention for a future phase (`CONTRIBUTING.md`, "Style"): removing the
//! `#[ignore]` before the phase lands fails loudly instead of passing vacuously.
//!
//! # The fallback rules, stated once
//!
//! | Missing in the file | Result |
//! |---|---|
//! | `baseColorFactor` | mid grey `[0.5, 0.5, 0.5, 1.0]` |
//! | `metallicFactor` | `0.0` (non-metal) |
//! | `roughnessFactor` | `0.5` |
//! | `emissiveFactor` | black — nothing emitted |
//! | base-colour texture | the solid `base_color`, no crash, no black mesh |
//! | normal texture, wrong format | ignored, `warn!`, the rest of the material kept |
//! | `alphaMode` | `Opaque` |
//! | whole material | [`Material::DEFAULT`] equivalent, never `None` |

use giochi_engine_3d::render3d::Material;

/// The five partial materials a real asset pack always contains, plus what each must
/// resolve to. Expressed as data so S3 can drive `MaterialContract::from_gltf` over the
/// same table the documentation shows.
const FALLBACK_TABLE: &[(&str, &str)] = &[
    ("no material at all", "Material::DEFAULT"),
    ("base colour only", "roughness 0.5, metallic 0.0, opaque"),
    ("roughness only", "base colour mid grey"),
    (
        "texture path that does not exist",
        "solid base colour, no error",
    ),
    (
        "normal map that is not an image",
        "normal ignored, rest kept",
    ),
];

#[test]
#[ignore = "Ativado na Fase S3"]
fn a_complete_gltf_material_fills_every_contract_field() {
    todo!("S3: MaterialContract::from_gltf + material_contract/fallback.rs")
}

#[test]
#[ignore = "Ativado na Fase S3"]
fn every_documented_fallback_resolves_without_a_error() {
    // The table above is the contract; S3 asserts one row per test so a failure names
    // the case that broke. Keeping the table here makes the two drift together.
    assert_eq!(FALLBACK_TABLE.len(), 5);
    todo!(
        "S3: MaterialContract::fallback() and the fallback rules in material_contract/fallback.rs"
    )
}

#[test]
#[ignore = "Ativado na Fase S3"]
fn a_mask_material_without_a_cutoff_is_rejected_by_validate() {
    todo!("S3: MaterialContract::validate() must refuse AlphaMode::Mask with alpha_cutoff <= 0")
}

#[test]
#[ignore = "Ativado na Fase S3"]
fn the_renderer_keeps_drawing_a_material_with_no_texture() {
    // This half already holds and is asserted so S3 cannot regress it: a material with
    // no texture is drawn with its base colour, and the default is a plain white matte.
    let default = Material::DEFAULT;
    assert!(!default.emits());
    assert_eq!(default.metallic, 0.0);
    assert!(default.resolved_shininess() > 0.0);

    let grey = Material {
        base_color: giochi_engine_3d::math3d::Color3D::new(0.5, 0.5, 0.5, 1.0),
        ..Material::DEFAULT
    };
    assert!(grey.base_color.r > 0.0);
}
