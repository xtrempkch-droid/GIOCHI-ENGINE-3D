#version 100
// Flat colour fragment shader.
//
// `mediump` is the safe default for fragment work on GLES 2.0 embedded drivers;
// `highp` is optional there and may be rejected.
precision mediump float;

uniform vec4 u_color;

varying vec3 v_normal;

void main() {
    // A single directional term keeps the cube readable without a full lighting model
    // (that is phase 13). Without it an unlit cube is a flat silhouette and the
    // rotation is invisible.
    vec3 normal = normalize(v_normal);
    float lambert = max(dot(normal, normalize(vec3(0.4, 0.9, 0.3))), 0.0);
    gl_FragColor = vec4(u_color.rgb * (0.35 + 0.65 * lambert), u_color.a);
}
