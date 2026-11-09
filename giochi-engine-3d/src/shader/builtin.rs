//! Built-in shaders, compiled into the binary.
//!
//! R10 of the 3D extension: shader source lives in `shaders/*.vert`/`*.frag`, never as
//! a string literal in `renderer.rs`. The built-ins are `include_str!`ed, so a typo in
//! GLSL is caught the first time the shader is used and the driver log names the file's
//! line — instead of a `\n`-escaped blob inside a Rust string.

/// The shaders this crate ships with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinShader {
    /// Solid colour with one directional light term. The phase 11 default.
    Flat,
    /// `texture2D` sampling with the same light term.
    Textured,
    /// Directional Blinn-Phong: ambient, diffuse and specular, all driven by the
    /// uniforms of [`DirectionalLight`](crate::DirectionalLight). The phase 13
    /// addition; nothing else in the crate depends on it, so `flat` keeps working
    /// unchanged on a driver with a tight uniform budget.
    Phong,
    /// [`BuiltinShader::Phong`] with a per-instance model matrix, for
    /// [`RenderContext3D::draw_instanced`](crate::RenderContext3D::draw_instanced).
    ///
    /// Same fragment shader as `Phong`, so an instanced cube and a `Phong` cube lit by
    /// the same sun are pixel-identical. The vertex shader reads the model matrix from
    /// attributes 3..6 instead of `u_model`.
    Instanced,
}

impl BuiltinShader {
    /// Every built-in, for tests and for `--help` output.
    pub const ALL: [BuiltinShader; 4] = [
        BuiltinShader::Flat,
        BuiltinShader::Textured,
        BuiltinShader::Phong,
        BuiltinShader::Instanced,
    ];

    /// A stable name, used in logs and errors.
    pub fn name(self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::Textured => "textured",
            Self::Phong => "phong",
            Self::Instanced => "instanced",
        }
    }

    /// The vertex source.
    pub fn vertex_source(self) -> &'static str {
        match self {
            Self::Flat => include_str!("../../shaders/flat.vert"),
            Self::Textured => include_str!("../../shaders/textured.vert"),
            Self::Phong => include_str!("../../shaders/phong.vert"),
            Self::Instanced => include_str!("../../shaders/instanced.vert"),
        }
    }

    /// The fragment source.
    pub fn fragment_source(self) -> &'static str {
        match self {
            Self::Flat => include_str!("../../shaders/flat.frag"),
            Self::Textured => include_str!("../../shaders/textured.frag"),
            // Deliberately the same fragment source: an instanced scene must shade
            // exactly like the non-instanced one, or the two arms of the phase-17
            // benchmark would not be drawing the same picture.
            Self::Phong | Self::Instanced => include_str!("../../shaders/phong.frag"),
        }
    }

    /// Whether this shader reads the lighting uniforms.
    ///
    /// Callers that want lighting must pick a shader that reads it; uploading light
    /// uniforms to `flat` is not an error (an unknown uniform is skipped) but is
    /// silently nothing, so it is worth being able to ask.
    pub fn reads_lighting(self) -> bool {
        matches!(self, Self::Phong | Self::Instanced)
    }

    /// Whether the shader takes its model matrix from per-instance attributes.
    ///
    /// `draw_instanced` requires it and the plain `draw_mesh` paths refuse it, so a
    /// mismatch is reported instead of producing N copies stacked at the origin.
    pub fn is_instanced(self) -> bool {
        matches!(self, Self::Instanced)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_starts_with_version_100() {
        // GLSL ES 1.00 requires `#version 100` as the first line. A `#version 300 es`
        // here would fail on the GLES 2.0 context this engine requests by default
        // (ADR-038), and the failure would only show at runtime.
        for shader in BuiltinShader::ALL {
            for (stage, source) in [
                ("vertex", shader.vertex_source()),
                ("fragment", shader.fragment_source()),
            ] {
                let first_line = source.lines().next().unwrap_or_default().trim();
                assert_eq!(
                    first_line,
                    "#version 100",
                    "{} {stage} shader must start with #version 100, got {first_line:?}",
                    shader.name()
                );
            }
        }
    }

    #[test]
    fn every_builtin_declares_a_precision() {
        // GLES fragment shaders have no default float precision; omitting it is a
        // compile error on strict drivers and a silent mediump elsewhere.
        for shader in BuiltinShader::ALL {
            assert!(
                shader
                    .fragment_source()
                    .contains("precision mediump float;"),
                "{} fragment shader needs a precision qualifier",
                shader.name()
            );
        }
        for shader in BuiltinShader::ALL {
            assert!(
                shader.vertex_source().contains("precision highp float;"),
                "{} vertex shader needs highp positions",
                shader.name()
            );
        }
    }

    #[test]
    fn no_builtin_uses_gles3_only_syntax() {
        // Instancing, uniform blocks, `layout(location=)` and `texture()` instead of
        // `texture2D()` are 3.0-only. Catching them here is cheaper than a failed
        // context on a device.
        //
        // Comments are stripped first: the word "without" in a comment contains "out ",
        // which the first version of this test flagged as GLSL 3 syntax.
        for shader in BuiltinShader::ALL {
            for source in [shader.vertex_source(), shader.fragment_source()] {
                let code: String = source
                    .lines()
                    .map(|line| match line.find("//") {
                        Some(at) => &line[..at],
                        None => line,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                for forbidden in ["uniform block", "layout(", "texture(", "gl_InstanceID"] {
                    assert!(
                        !code.contains(forbidden),
                        "{} uses GLES 3.0 syntax {forbidden:?}",
                        shader.name()
                    );
                }

                // `in`/`out` as *declaration keywords* only: at the start of a trimmed
                // line, followed by a type-ish word.
                for line in code.lines() {
                    let trimmed = line.trim_start();
                    for keyword in ["in ", "out "] {
                        assert!(
                            !trimmed.starts_with(keyword),
                            "{} declares {{trimmed:?}}; GLES 2.0 uses attribute/varying",
                            shader.name()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_attribute_names_match_the_vertex_layout() {
        // These names are looked up by string when the VAO is built; a rename here
        // without a rename in `mesh.rs` silently disables the attribute.
        for shader in BuiltinShader::ALL {
            assert!(shader.vertex_source().contains("a_position"));
            assert!(shader.vertex_source().contains("a_normal"));
        }
    }

    #[test]
    fn names_are_unique() {
        let names: Vec<&str> = BuiltinShader::ALL.iter().map(|s| s.name()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();

        assert_eq!(sorted.len(), names.len(), "duplicate built-in names");
    }

    #[test]
    fn the_phong_shader_declares_every_uniform_the_renderer_uploads() {
        // The renderer uploads these names on every draw. A typo on either side is a
        // missing uniform, which GL reports as nothing at all -- `get_uniform_location`
        // returns null and `set_*` silently skips it. The scene then renders unlit with
        // no error anywhere, so the names are pinned here.
        /// Whether `source` has a declaration line for `name`.
        ///
        /// Matching a whole line rather than a substring keeps a name mentioned inside a
        /// comment from satisfying the test.
        fn declares(source: &str, name: &str) -> bool {
            source
                .lines()
                .any(|line| line.trim_start().starts_with("uniform ") && line.contains(name))
        }

        let fragment = BuiltinShader::Phong.fragment_source();
        for uniform in [
            "u_color",
            "u_light_direction",
            "u_light_color",
            "u_ambient_strength",
            "u_specular_strength",
            "u_shininess",
            "u_camera_position",
        ] {
            assert!(
                declares(fragment, uniform),
                "the phong fragment shader must declare {uniform}"
            );
        }

        let vertex = BuiltinShader::Phong.vertex_source();
        for uniform in ["u_view_proj", "u_model", "u_normal_matrix"] {
            assert!(
                declares(vertex, uniform),
                "the phong vertex shader must declare {uniform}"
            );
        }
    }

    #[test]
    fn the_phong_shader_interpolates_world_space_varyings() {
        // View-space normals make the light follow the camera. The varyings are named
        // `world` for that reason, and this test fails if someone "simplifies" them back
        // into view space without renaming.
        let vertex = BuiltinShader::Phong.vertex_source();
        let fragment = BuiltinShader::Phong.fragment_source();

        assert!(vertex.contains("v_world_normal"));
        assert!(vertex.contains("v_world_position"));
        assert!(fragment.contains("v_world_normal"));
        assert!(fragment.contains("v_world_position"));

        // Both stages must agree on the varying names or linking fails at runtime.
        for varying in ["v_world_normal", "v_world_position"] {
            assert_eq!(
                vertex.matches(varying).count() > 0,
                fragment.matches(varying).count() > 0,
                "{varying} must be declared in both stages"
            );
        }
    }

    #[test]
    fn only_the_phong_shader_reads_the_lighting_uniforms() {
        assert!(BuiltinShader::Phong.reads_lighting());
        assert!(!BuiltinShader::Flat.reads_lighting());
        assert!(!BuiltinShader::Textured.reads_lighting());
    }
}
