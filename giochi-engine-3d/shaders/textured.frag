#version 100
// Textured fragment shader.
precision mediump float;

uniform sampler2D u_texture;
uniform vec4 u_tint;

varying vec2 v_uv;
varying vec3 v_normal;

void main() {
    vec4 sample_color = texture2D(u_texture, v_uv);

    // The same single directional term as the flat shader, so a textured cube is not a
    // silhouette either.
    vec3 normal = normalize(v_normal);
    float lambert = max(dot(normal, normalize(vec3(0.4, 0.9, 0.3))), 0.0);

    gl_FragColor = sample_color * u_tint * (0.35 + 0.65 * lambert);
}
