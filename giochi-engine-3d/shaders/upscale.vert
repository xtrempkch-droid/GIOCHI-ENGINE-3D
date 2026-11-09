#version 100
// Fullscreen-quad vertex shader for the upscale pass.
//
// Positions arrive already in clip space (`-1..1`), which is why there is no camera,
// no view-projection and no model matrix: this pass does not draw a scene, it copies a
// texture across the whole surface.
//
// GLSL ES 1.00 has no `gl_VertexID`, so the quad comes from a four-float VBO rather
// than being generated. That is the one piece of ceremony GLES 3.0 would save.
precision highp float;

attribute vec2 a_position;
attribute vec2 a_uv;

varying vec2 v_uv;

void main() {
    v_uv = a_uv;
    gl_Position = vec4(a_position, 0.0, 1.0);
}
