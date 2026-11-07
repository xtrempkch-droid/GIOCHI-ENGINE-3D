#version 100
// Directional Phong fragment shader for GLES 2.0.
//
// One infinitely distant light: ambient + diffuse + specular. No attenuation to model,
// because a directional light has none, and no per-pixel loops, which is what keeps this
// legal on an embedded GLES 2.0 driver.
precision mediump float;

uniform vec4 u_color;
uniform vec3 u_light_direction;
uniform vec3 u_light_color;
uniform float u_ambient_strength;
uniform float u_specular_strength;
uniform float u_shininess;
uniform vec3 u_camera_position;

varying vec3 v_world_normal;
varying vec3 v_world_position;

void main() {
    vec3 normal = normalize(v_world_normal);

    // `u_light_direction` points the way the light travels, so the vector towards the
    // light is its negation. Getting this backwards produces a scene lit from below,
    // which reads as "the sun is under the floor" rather than as a sign error.
    vec3 to_light = normalize(-u_light_direction);
    vec3 to_eye = normalize(u_camera_position - v_world_position);

    float diffuse = max(dot(normal, to_light), 0.0);

    // Blinn-Phong: the half vector between the light and the eye. One `normalize`
    // instead of a `reflect`, and it stays stable when the eye sits near the mirror
    // direction, where `pow(dot(n, reflect(..)), n)` degenerates.
    vec3 half_vector = normalize(to_light + to_eye);
    float specular = 0.0;
    if (diffuse > 0.0) {
        specular = pow(max(dot(normal, half_vector), 0.0), u_shininess);
    }

    vec3 ambient = u_color.rgb * u_ambient_strength;
    vec3 direct = u_color.rgb * u_light_color * diffuse;
    vec3 highlight = u_light_color * specular * u_specular_strength;

    gl_FragColor = vec4(ambient + direct + highlight, u_color.a);
}
