#version 100
// Flat (unlit) vertex shader for GLES 2.0.
//
// GLSL ES 1.00: `#version 100` must be the very first line, and a vertex shader needs
// the default highp precision for positions or the driver rejects it on some ARM
// implementations.
precision highp float;

uniform mat4 u_view_proj;
uniform mat4 u_model;

attribute vec3 a_position;
attribute vec3 a_normal;

varying vec3 v_normal;

void main() {
    vec4 world = u_model * vec4(a_position, 1.0);
    v_normal = mat3(u_model) * a_normal;
    gl_Position = u_view_proj * world;
}
