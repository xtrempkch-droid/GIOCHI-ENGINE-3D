#version 100
// Textured vertex shader for GLES 2.0.
precision highp float;

uniform mat4 u_view_proj;
uniform mat4 u_model;

attribute vec3 a_position;
attribute vec3 a_normal;
attribute vec2 a_uv;

varying vec2 v_uv;
varying vec3 v_normal;

void main() {
    vec4 world = u_model * vec4(a_position, 1.0);
    v_uv = a_uv;
    v_normal = mat3(u_model) * a_normal;
    gl_Position = u_view_proj * world;
}
