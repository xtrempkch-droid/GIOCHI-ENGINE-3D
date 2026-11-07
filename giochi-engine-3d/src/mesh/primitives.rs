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

/// Vertices and indices of a UV sphere centred on the origin.
///
/// `radius` is the sphere's radius (so it is a *ball*, and the call site can name it
/// that), `rings` the number of latitude bands and `segments` the longitude divisions.
/// More rings than segments gives the poles less pinching for the same triangle count.
///
/// Normals are the normalised position: on a sphere the outward direction *is* the
/// radial vector, which makes the flat-lit version read as a ball rather than as a
/// faceted polyhedron.
pub fn sphere(radius: f32, rings: u32, segments: u32) -> (Vec<Vertex>, Vec<u32>) {
    let rings = rings.max(2);
    let segments = segments.max(3);

    let mut vertices = Vec::with_capacity(((rings + 1) * (segments + 1)) as usize);

    for ring in 0..=rings {
        // 0 at the north pole, PI at the south.
        let polar = std::f32::consts::PI * ring as f32 / rings as f32;
        let (sin_polar, cos_polar) = polar.sin_cos();

        for segment in 0..=segments {
            let azimuth = std::f32::consts::TAU * segment as f32 / segments as f32;
            let (sin_azimuth, cos_azimuth) = azimuth.sin_cos();

            let normal = [sin_polar * cos_azimuth, cos_polar, sin_polar * sin_azimuth];
            let position = [normal[0] * radius, normal[1] * radius, normal[2] * radius];
            // Equirectangular UV: `u` around, `v` from the south pole up.
            let uv = [
                segment as f32 / segments as f32,
                1.0 - ring as f32 / rings as f32,
            ];

            vertices.push(Vertex::new(position, normal, uv));
        }
    }

    // Two triangles per quad, counter-clockwise seen from outside.
    let mut indices = Vec::with_capacity((rings * segments * 6) as usize);
    let stride = segments + 1;
    for ring in 0..rings {
        for segment in 0..segments {
            let north_west = ring * stride + segment;
            let north_east = north_west + 1;
            let south_west = north_west + stride;
            let south_east = south_west + 1;

            // Order chosen so the triangle winds counter-clockwise seen from *outside*
            // the sphere: `sin(azimuth)` grows toward +Z, which is to the left when
            // looking inward along -X, so the naive quad order points every normal the
            // wrong way.
            //
            // The two guards are not symmetric, and getting them backwards leaves a
            // band of zero-area triangles at the north pole: at `ring == 0` the first two
            // vertices coincide, and at the last ring the last two do.
            if ring + 1 != rings {
                indices.extend_from_slice(&[north_west, south_east, south_west]);
            }
            if ring != 0 {
                indices.extend_from_slice(&[north_west, north_east, south_east]);
            }
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
    fn a_sphere_has_the_expected_vertex_count() {
        let (vertices, _) = sphere(0.5, 8, 12);

        // (rings + 1) * (segments + 1): the duplicated seam column keeps the UVs sane.
        assert_eq!(vertices.len(), 9 * 13);
    }

    #[test]
    fn every_sphere_vertex_sits_on_the_radius_and_its_normal_is_radial() {
        let radius = 1.75;
        let (vertices, _) = sphere(radius, 6, 8);

        for vertex in &vertices {
            let [x, y, z] = vertex.position;
            let length = (x * x + y * y + z * z).sqrt();
            assert!(
                (length - radius).abs() < 1e-5,
                "vertex is at {length}, expected {radius}"
            );

            // Outward normal: the position and normal must point the same way.
            let dot = x * vertex.normal[0] + y * vertex.normal[1] + z * vertex.normal[2];
            assert!(dot > 0.0, "normal points inward: {vertex:?}");
        }
    }

    #[test]
    fn every_sphere_index_is_in_range() {
        let (vertices, indices) = sphere(1.0, 5, 7);

        for index in &indices {
            assert!(
                (*index as usize) < vertices.len(),
                "index {index} out of range"
            );
        }
        assert!(!indices.is_empty());
    }

    #[test]
    fn a_sphere_winds_counter_clockwise_seen_from_outside() {
        let (vertices, indices) = sphere(1.0, 6, 8);

        let mut checked = 0;
        for triangle in indices.chunks_exact(3) {
            let p0 = vertices[triangle[0] as usize].position;
            let p1 = vertices[triangle[1] as usize].position;
            let p2 = vertices[triangle[2] as usize].position;

            let u = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let v = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];

            // The centroid is inside the sphere, so an outward winding has a positive
            // dot with the centroid direction.
            let centroid = [
                (p0[0] + p1[0] + p2[0]) / 3.0,
                (p0[1] + p1[1] + p2[1]) / 3.0,
                (p0[2] + p1[2] + p2[2]) / 3.0,
            ];
            let dot = cross[0] * centroid[0] + cross[1] * centroid[1] + cross[2] * centroid[2];
            assert!(dot > 0.0, "triangle {triangle:?} winds inward");
            checked += 1;
        }
        assert!(checked > 20, "only {checked} triangles checked");
    }

    #[test]
    fn a_sphere_with_silly_subdivisions_still_produces_geometry() {
        let (vertices, indices) = sphere(1.0, 0, 0);

        assert!(vertices.len() >= 9, "rings and segments are clamped");
        assert!(!indices.is_empty());
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
