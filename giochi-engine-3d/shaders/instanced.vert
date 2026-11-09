#version 100
// Instanced Phong vertex shader for GLES 2.0.
//
// One draw call shades N copies of the same mesh. The per-vertex attributes
// (`a_position`, `a_normal`) are shared by every instance; the per-instance attributes
// (`a_model_0`..`a_model_3`) advance once per instance.
//
// # Why the matrix arrives as four `vec4`
//
// GLSL ES 1.00 has no `mat4` vertex attribute. The GL API uploads a matrix as four
// consecutive `vec4` slots and the shader reassembles it. **Column-major**: the four
// slots are the matrix's columns, which is what `Mat4` already stores and what
// `glUniformMatrix4fv(..., GL_FALSE, ...)` expects — uploading rows instead would
// transpose every instance.
//
// # Attribute budget
//
// GLES 2.0 guarantees eight vertex attribute slots. Three are mesh attributes
// (position, normal, uv) and four are the instance matrix: seven, one spare. The
// phase-17 plan warned that a sixteenth-of-a-KB-per-instance stride could hit a low
// attribute limit and suggested packing a quaternion plus a position (7 floats)
// instead; that is not needed at eight slots, and reconstructing a matrix in the vertex
// shader would cost more per vertex than the slot it saved.
//
// `u_model` is deliberately absent. An instanced draw has no single model transform,
// so leaving the uniform out makes "you forgot to apply the instance matrix" a
// compile-time error rather than a scene where every copy sits at the origin.
precision highp float;

uniform mat4 u_view_proj;

attribute vec3 a_position;
attribute vec3 a_normal;
// The instance matrix, one column per attribute slot.
attribute vec4 a_model_0;
attribute vec4 a_model_1;
attribute vec4 a_model_2;
attribute vec4 a_model_3;

varying vec3 v_world_normal;
varying vec3 v_world_position;

void main() {
    mat4 model = mat4(a_model_0, a_model_1, a_model_2, a_model_3);
    vec4 world = model * vec4(a_position, 1.0);

    v_world_position = world.xyz;
    // The instance transform is translation + rotation + uniform scale by construction
    // (`Transform3D` / `Mat4` here never carry a non-uniform scale), so the upper 3x3 is
    // the normal matrix up to that uniform scale, which `normalize` in the fragment
    // shader absorbs. A shear would need a real inverse-transpose, and no path here
    // produces one.
    v_world_normal = mat3(model) * a_normal;

    gl_Position = u_view_proj * world;
}
