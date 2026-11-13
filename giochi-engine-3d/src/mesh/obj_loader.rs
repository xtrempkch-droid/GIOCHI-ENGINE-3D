//! Wavefront OBJ loading via `tobj`.
//!
//! `tobj` hands back three flat `Vec<f32>` (positions, normals, uvs) and an index list;
//! this module interleaves them into [`Vertex`]s. Two adjustments matter:
//!
//! * **Missing normals.** An OBJ without `vn` records parses fine and produces an empty
//!   normals buffer. Leaving them at zero gives a black model with no error, so a face
//!   normal is computed from the triangle instead.
//! * **Missing UVs.** Likewise, `vt` is optional; the vertex gets `(0, 0)` and the
//!   textured shader samples a single texel, which is visible but not a crash.

use std::path::Path;

use super::{prepare_normals, Vertex};
use crate::error::{Error3D, Result3D};

/// Loads every mesh in `path` and concatenates them into one vertex/index pair.
///
/// OBJ files with several `o`/`g` objects are merged: the phase 11 pipeline draws one
/// mesh per call, and a model that silently drew only its first object would be a
/// confusing half-model.
///
/// # Errors
///
/// [`Error3D::Asset`] when the file cannot be read or parsed. The message keeps the
/// path and the parser's own text.
pub fn load_obj(path: &Path) -> Result3D<(Vec<Vertex>, Vec<u32>)> {
    if !path.exists() {
        return Err(Error3D::Asset(format!(
            "OBJ file does not exist: {}",
            path.display()
        )));
    }

    let options = tobj::LoadOptions {
        // Single-index OBJ files are common; `tobj` otherwise refuses them.
        single_index: false,
        triangulate: true,
        ignore_points: true,
        ignore_lines: true,
    };

    let (models, _materials) = tobj::load_obj(path, &options).map_err(|err| {
        Error3D::Asset(format!("failed to parse OBJ '{}': {err}", path.display()))
    })?;

    if models.is_empty() {
        return Err(Error3D::Asset(format!(
            "OBJ file '{}' contains no mesh",
            path.display()
        )));
    }

    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for model in &models {
        let mesh = &model.mesh;
        append_mesh(path, mesh, &mut vertices, &mut indices)?;
    }

    if vertices.is_empty() || indices.is_empty() {
        return Err(Error3D::Asset(format!(
            "OBJ file '{}' produced no geometry",
            path.display()
        )));
    }

    // One pass over the merged buffer instead of per object: it normalises the `vn` the
    // file carried and derives the ones it did not. Doing it after the merge keeps the
    // index rebasing above simple, and a vertex only ever belongs to triangles of its own
    // object, so the result is the same.
    prepare_normals(&mut vertices, &indices);

    Ok((vertices, indices))
}

fn append_mesh(
    path: &Path,
    mesh: &tobj::Mesh,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) -> Result3D<()> {
    let base = vertices.len() as u32;
    let count = mesh.positions.len() / 3;

    // `prepare_normals` runs after the merge and derives whatever is missing or unusable,
    // so a `vn` that is absent, `NaN` or zero becomes a face normal there. A non-finite
    // *position* cannot be recovered from, however: it cannot be projected, bounded or
    // used to derive a normal, so the file is refused with enough detail to find it.
    let has_normals = mesh.normals.len() >= count * 3;
    let has_uvs = mesh.texcoords.len() >= count * 2;

    for i in 0..count {
        let position = [
            mesh.positions[i * 3],
            mesh.positions[i * 3 + 1],
            mesh.positions[i * 3 + 2],
        ];

        if !position.iter().all(|value| value.is_finite()) {
            return Err(Error3D::Asset(format!(
                "OBJ file '{}' has a non-finite vertex position at index {i}",
                path.display()
            )));
        }

        let normal = if has_normals {
            [
                mesh.normals[i * 3],
                mesh.normals[i * 3 + 1],
                mesh.normals[i * 3 + 2],
            ]
        } else {
            // Replaced by a derived face normal in `prepare_normals`.
            [0.0, 0.0, 0.0]
        };

        let uv = if has_uvs {
            [mesh.texcoords[i * 2], mesh.texcoords[i * 2 + 1]]
        } else {
            [0.0, 0.0]
        };

        vertices.push(Vertex::new(position, normal, uv));
    }

    indices.extend(mesh.indices.iter().map(|index| base + index));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_an_asset_error_naming_the_path() {
        let error =
            load_obj(Path::new("/nonexistent/definitely/not/here.obj")).expect_err("must fail");
        let text = error.to_string();

        assert!(text.contains("not/here.obj"), "{text}");
    }

    #[test]
    fn a_non_obj_file_is_rejected_without_panicking() {
        // `tobj` parses some garbage without complaint, so this exercises the
        // "no geometry" path rather than the parser's.
        let path = std::env::temp_dir().join("giochi-not-an-obj.txt");
        std::fs::write(&path, b"this is not a wavefront obj file\n").expect("write");

        let result = load_obj(&path);
        let _ = std::fs::remove_file(&path);

        assert!(matches!(result, Err(Error3D::Asset(_))), "got {result:?}");
    }

    #[test]
    fn a_minimal_obj_produces_one_triangle_with_derived_normals() {
        // Four floats per line, no `vn` and no `vt`: the loader must fill in both.
        let path = std::env::temp_dir().join("giochi-minimal.obj");
        let source = "\
o triangle
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
f 1 2 3
";
        std::fs::write(&path, source).expect("write");
        let result = load_obj(&path);
        let _ = std::fs::remove_file(&path);

        let (vertices, indices) = result.expect("a valid OBJ");
        assert_eq!(vertices.len(), 3);
        assert_eq!(indices, vec![0, 1, 2]);

        // The triangle lies in the XY plane, so every derived normal is +Z.
        for vertex in &vertices {
            let [x, y, z] = vertex.normal;
            let length = (x * x + y * y + z * z).sqrt();
            assert!((length - 1.0).abs() < 1e-5, "normal not unit: {vertex:?}");
            assert!(z.abs() > 0.99, "expected +Z normal, got {vertex:?}");
        }
    }

    #[test]
    fn a_nan_normal_is_treated_as_missing_and_derived() {
        // A malformed `vn` used to pass straight through: `Vertex::normalized` leaves a
        // non-finite normal untouched, so it reached the shader and shaded the model
        // black with no error at all.
        let path = std::env::temp_dir().join("giochi-nan-normal.obj");
        let source = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
vn NaN NaN NaN
f 1//1 2//1 3//1
";
        std::fs::write(&path, source).expect("write");
        let result = load_obj(&path);
        let _ = std::fs::remove_file(&path);

        let (vertices, _) = result.expect("the file is still loadable");
        for vertex in &vertices {
            assert!(
                vertex.normal.iter().all(|value| value.is_finite()),
                "a NaN normal must be replaced, got {vertex:?}"
            );
            assert!(vertex.normal[2].abs() > 0.99, "derived +Z normal expected");
        }
    }

    #[test]
    fn a_non_finite_position_is_refused_with_the_file_name() {
        // It cannot be projected or bounded, and it poisons every derived normal, so
        // the file is refused instead of producing an empty frame.
        let path = std::env::temp_dir().join("giochi-nan-position.obj");
        let source = "\
v 0.0 0.0 0.0
v inf 0.0 0.0
v 0.0 1.0 0.0
f 1 2 3
";
        std::fs::write(&path, source).expect("write");
        let result = load_obj(&path);
        let _ = std::fs::remove_file(&path);

        match result {
            Err(Error3D::Asset(message)) => {
                assert!(message.contains("non-finite"), "{message}");
                assert!(message.contains("nan-position"), "{message}");
            }
            other => panic!("expected an asset error, got {other:?}"),
        }
    }

    #[test]
    fn an_obj_with_two_objects_is_merged() {
        let path = std::env::temp_dir().join("giochi-two-objects.obj");
        let source = "\
o first
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
f 1 2 3
o second
v 0.0 0.0 1.0
v 1.0 0.0 1.0
v 0.0 1.0 1.0
f 4 5 6
";
        std::fs::write(&path, source).expect("write");
        let result = load_obj(&path);
        let _ = std::fs::remove_file(&path);

        let (vertices, indices) = result.expect("a valid OBJ");
        assert_eq!(vertices.len(), 6, "two triangles");
        assert_eq!(indices.len(), 6);
        // The second object's indices must be rebased, or it would re-draw the first.
        assert_eq!(indices, vec![0, 1, 2, 3, 4, 5]);
    }
}
