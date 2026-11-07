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
}

impl BuiltinShader {
    /// Every built-in, for tests and for `--help` output.
    pub const ALL: [BuiltinShader; 2] = [BuiltinShader::Flat, BuiltinShader::Textured];

    /// A stable name, used in logs and errors.
    pub fn name(self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::Textured => "textured",
        }
    }

    /// The vertex source.
    pub fn vertex_source(self) -> &'static str {
        match self {
            Self::Flat => include_str!("../../shaders/flat.vert"),
            Self::Textured => include_str!("../../shaders/textured.vert"),
        }
    }

    /// The fragment source.
    pub fn fragment_source(self) -> &'static str {
        match self {
            Self::Flat => include_str!("../../shaders/flat.frag"),
            Self::Textured => include_str!("../../shaders/textured.frag"),
        }
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
}
