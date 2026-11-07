//! Procedurally generated meshes.
//!
//! The example uses these instead of a committed `.obj`: a cube is 24 vertices of
//! arithmetic, and shipping a binary asset to CI (and to the release tarball) to prove
//! "the pipeline draws triangles" would add a failure mode without adding coverage.
//!
//! [`cube`] duplicates the eight corners into 24 vertices on purpose. Sharing them
//! would force one normal per corner, so every face would be lit as if it were the same
//! plane — a cube that looks like a sphere shaded flat.

use super::Vertex;

/// Vertices and indices of an axis-aligned cube centred on the origin.
///
/// `size` is the length of one edge. Six faces, four vertices each, counter-clockwise
/// winding seen from outside (GL's default front face, `GlState::front_face`).
pub fn cube(size: f32) -> (Vec<Vertex>, Vec<u32>) {
    let half = size * 0.5;

    // (normal, four corners in counter-clockwise order seen from outside)
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        // +X. Seen from +X the local "right" is -Z, so bottom-left is +Z.
        (
            [1.0, 0.0, 0.0],
            [
                [half, -half, half],
                [half, -half, -half],
                [half, half, -half],
                [half, half, half],
            ],
        ),
        // -X. Seen from -X the local "right" is +Z.
        (
            [-1.0, 0.0, 0.0],
            [
                [-half, -half, -half],
                [-half, -half, half],
                [-half, half, half],
                [-half, half, -half],
            ],
        ),
        // +Y. Seen from above the local "right" is +X and "up" is -Z.
        (
            [0.0, 1.0, 0.0],
            [
                [-half, half, half],
                [half, half, half],
                [half, half, -half],
                [-half, half, -half],
            ],
        ),
        // -Y
        (
            [0.0, -1.0, 0.0],
            [
                [-half, -half, -half],
                [half, -half, -half],
                [half, -half, half],
                [-half, -half, half],
            ],
        ),
        // +Z
        (
            [0.0, 0.0, 1.0],
            [
                [-half, -half, half],
                [half, -half, half],
                [half, half, half],
                [-half, half, half],
            ],
        ),
        // -Z
        (
            [0.0, 0.0, -1.0],
            [
                [half, -half, -half],
                [-half, -half, -half],
                [-half, half, -half],
                [half, half, -half],
            ],
        ),
    ];

    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);

    for (normal, corners) in faces {
        let base = vertices.len() as u32;

        // UVs cover the whole face: (0,0) bottom-left to (1,1) top-right.
        for (corner, uv) in corners
            .iter()
            .zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
        {
            vertices.push(Vertex::new(*corner, normal, uv));
        }

        // Two triangles, counter-clockwise: 0-1-2 and 0-2-3.
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    (vertices, indices)
}

/// Vertices and indices of a horizontal plane centred on the origin.
///
/// `size` is the length of one edge, `subdivisions` how many quads per side. One quad
/// would be enough for a flat colour; more matter once phase 13 lights it, because a
/// single quad interpolates lighting across the whole floor.
pub fn plane(size: f32, subdivisions: u32) -> (Vec<Vertex>, Vec<u32>) {
    let steps = subdivisions.max(1);
    let half = size * 0.5;
    let step = size / steps as f32;

    let mut vertices = Vec::with_capacity(((steps + 1) * (steps + 1)) as usize);
    for row in 0..=steps {
        for column in 0..=steps {
            let x = -half + column as f32 * step;
            let z = -half + row as f32 * step;
            let u = column as f32 / steps as f32;
            let v = row as f32 / steps as f32;
            // +Y normal, counter-clockwise seen from above.
            vertices.push(Vertex::new([x, 0.0, z], [0.0, 1.0, 0.0], [u, v]));
        }
    }

    let mut indices = Vec::with_capacity((steps * steps * 6) as usize);
    let stride = steps + 1;
    for row in 0..steps {
        for column in 0..steps {
            let top_left = row * stride + column;
            let top_right = top_left + 1;
            let bottom_left = top_left + stride;
            let bottom_right = bottom_left + 1;

            // Counter-clockwise from above (+Y): this winding is what makes the floor
            // visible with culling on.
            indices.extend_from_slice(&[
                top_left,
                bottom_left,
                bottom_right,
                top_left,
                bottom_right,
                top_right,
            ]);
        }
    }

    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cube_has_24_vertices_and_36_indices() {
        let (vertices, indices) = cube(2.0);

        assert_eq!(vertices.len(), 24, "four per face, six faces");
        assert_eq!(indices.len(), 36, "two triangles per face");
    }

    #[test]
    fn every_cube_index_is_in_range() {
        let (vertices, indices) = cube(1.0);

        for index in &indices {
            assert!(
                (*index as usize) < vertices.len(),
                "index {index} is out of range"
            );
        }
    }

    #[test]
    fn cube_normals_are_unit_and_face_outwards() {
        let (vertices, _) = cube(2.0);

        for vertex in &vertices {
            let [x, y, z] = vertex.normal;
            let length = (x * x + y * y + z * z).sqrt();
            assert!((length - 1.0).abs() < 1e-6, "normal is not unit");

            // Each vertex sits on the face its normal points away from: the dot of
            // position and normal is positive (half the edge).
            let dot = vertex.position[0] * x + vertex.position[1] * y + vertex.position[2] * z;
            assert!(dot > 0.0, "normal points into the cube: {vertex:?}");
        }
    }

    #[test]
    fn every_cube_face_winds_counter_clockwise_seen_from_outside() {
        // A clockwise face is culled and the cube looks hollow. Checking only the first
        // face is not enough: the first version of `cube` had two faces wound backwards
        // and this test found them.
        let (vertices, indices) = cube(2.0);

        for (face, triangle) in indices.chunks_exact(6).enumerate() {
            let p0 = vertices[triangle[0] as usize].position;
            let p1 = vertices[triangle[1] as usize].position;
            let p2 = vertices[triangle[2] as usize].position;
            let normal = vertices[triangle[0] as usize].normal;

            let u = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let v = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];

            // The computed geometric normal must agree with the declared face normal.
            let dot = cross[0] * normal[0] + cross[1] * normal[1] + cross[2] * normal[2];
            assert!(
                dot > 0.0,
                "face {face} (normal {normal:?}) winds clockwise; cross = {cross:?}"
            );
        }
    }

    #[test]
    fn a_plane_has_the_expected_vertex_and_index_counts() {
        let (vertices, indices) = plane(10.0, 4);

        assert_eq!(vertices.len(), 25, "5x5 grid");
        assert_eq!(indices.len(), 4 * 4 * 6, "16 quads, two triangles each");
    }

    #[test]
    fn a_plane_with_zero_subdivisions_is_clamped_to_one_quad() {
        let (vertices, indices) = plane(1.0, 0);

        assert_eq!(vertices.len(), 4);
        assert_eq!(indices.len(), 6);
    }

    #[test]
    fn plane_indices_are_in_range_and_normals_point_up() {
        let (vertices, indices) = plane(4.0, 3);

        for index in &indices {
            assert!((*index as usize) < vertices.len());
        }
        for vertex in &vertices {
            assert_eq!(vertex.normal, [0.0, 1.0, 0.0]);
            assert_eq!(vertex.position[1], 0.0, "the plane is horizontal");
        }
    }

    #[test]
    fn a_plane_face_winds_counter_clockwise_seen_from_above() {
        let (vertices, indices) = plane(2.0, 1);

        let p0 = vertices[indices[0] as usize].position;
        let p1 = vertices[indices[1] as usize].position;
        let p2 = vertices[indices[2] as usize].position;

        let u = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let v = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        let cross_y = u[2] * v[0] - u[0] * v[2];

        assert!(
            cross_y > 0.0,
            "the floor must wind counter-clockwise seen from +Y, got {cross_y}"
        );
    }
}
