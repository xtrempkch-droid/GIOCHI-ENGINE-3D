#version 100
// Bilinear upscale fragment shader.
//
// `texture2D` with `GL_LINEAR` filtering does the whole job: the four texels around the
// sample are weighted by distance in hardware. That is why this is four lines, and why
// it runs at full speed on a Mali-G31 that would struggle with anything more.
//
// # This is not FSR
//
// AMD's FidelityFX Super Resolution 1.0 needs a compute shader (an EASU edge-reconstruction
// pass and an RCAS sharpening pass). GLES 2.0 has no compute, and GLES 3.1 — the first
// version that does — is not available on this engine's ARM64 floor (ADR-038). FSR 2.x
// additionally needs motion vectors and temporal accumulation, i.e. a TAA pipeline that
// does not exist here.
//
// A bicubic 9-tap shader is reachable in GLES 2.0 and would be sharper than bilinear at
// the same cost class; it is not implemented because the acceptance criterion for phase
// 17c is a frame-rate gain, and every tap makes the pass slower on the device the gain
// is meant for. Bilinear is the honest trade: it buys the fill rate and it looks like a
// reduced-resolution image, which it is.
precision mediump float;

uniform sampler2D u_source;

varying vec2 v_uv;

void main() {
    gl_FragColor = texture2D(u_source, v_uv);
}
