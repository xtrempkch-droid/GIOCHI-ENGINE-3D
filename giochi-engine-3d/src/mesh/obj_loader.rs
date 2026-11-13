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

use super::Vertex;
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

    // A `vn` that is `NaN` or infinite is worse than a missing one: `Vertex::normalized`
    // deliberately leaves a non-finite normal alone, so it would reach the shader and
    // shade the whole model black with no error. Treat it as absent and derive the face
    // normals instead.
    let has_normals = mesh.normals.len() >= count * 3
        && mesh.normals[..count * 3]
            .iter()
            .all(|value| value.is_finite());
    let has_uvs = mesh.texcoords.len() >= count * 2;

    for i in 0..count {
        let position = [
            mesh.positions[i * 3],
            mesh.positions[i * 3 + 1],
            mesh.positions[i * 3 + 2],
        ];

        // A non-finite position cannot be drawn, projected or put in a bounding box, and
        // it propagates into every derived normal. Refusing the file names the problem
        // instead of showing an empty frame.
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
            [0.0, 0.0, 0.0]
        };

        let uv = if has_uvs {
            [mesh.texcoords[i * 2], mesh.texcoords[i * 2 + 1]]
        } else {
            [0.0, 0.0]
        };

        vertices.push(Vertex::new(position, normal, uv).normalized());
    }

    let first_new_index = indices.len();
    indices.extend(mesh.indices.iter().map(|index| base + index));

    if !has_normals {
        // With no normals in the file, derive them per triangle. This is the difference
        // between a lit model and a black silhouette.
        //
        // Only the indices just appended are walked: passing the whole list would make a
        // later object's triangles subtract a `base` larger than their own indices, which
        // underflows `usize`.
        compute_face_normals(vertices, &indices[first_new_index..], base, count);
    }

    Ok(())
}

/// Fills in normals by averaging the faces that share each vertex.
fn compute_face_normals(vertices: &mut [Vertex], indices: &[u32], base: u32, count: usize) {
    let mut accumulated = vec![[0.0_f32; 3]; count];

    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [
            triangle[0] as usize - base as usize,
            triangle[1] as usize - base as usize,
            triangle[2] as usize - base as usize,
        ];

        let p0 = vertices[base as usize + a].position;
        let p1 = vertices[base as usize + b].position;
        let p2 = vertices[base as usize + c].position;

        let u = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let v = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        let normal = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];

        for corner in [a, b, c] {
            if corner < count {
                accumulated[corner][0] += normal[0];
                accumulated[corner][1] += normal[1];
                accumulated[corner][2] += normal[2];
            }
        }
    }

    for (i, normal) in accumulated.into_iter().enumerate() {
        let vertex = &mut vertices[base as usize + i];
        if vertex.normal == [0.0, 0.0, 0.0] {
            vertex.normal = normal;
        }
        *vertex = vertex.normalized();
    }
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
