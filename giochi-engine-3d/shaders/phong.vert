#version 100
// Phong vertex shader for GLES 2.0.
//
// Everything the fragment shader needs is interpolated in **world space**, not view
// space. View space would make the light move with the camera, so the sun would appear
// to follow the player; world space lets one light direction describe the scene.
precision highp float;

uniform mat4 u_view_proj;
uniform mat4 u_model;
uniform mat4 u_normal_matrix;

attribute vec3 a_position;
attribute vec3 a_normal;

varying vec3 v_world_normal;
varying vec3 v_world_position;

void main() {
    vec4 world = u_model * vec4(a_position, 1.0);
    v_world_position = world.xyz;
    // `u_normal_matrix` is the inverse-transpose of the model's upper 3x3, computed on
    // the CPU: GLSL ES 1.00 has no `inverse()`. Using `mat3(u_model)` here would shade a
    // non-uniformly scaled object as though it had a different shape.
    v_world_normal = mat3(u_normal_matrix) * a_normal;
    gl_Position = u_view_proj * world;
}
