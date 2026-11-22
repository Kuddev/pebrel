// Versionless GLSL receives the background ABI and a mainImage entry wrapper.
void mainImage(out vec4 color, in vec2 pixel) {
    vec2 uv = pixel / iResolution.xy;
    float wave = 0.5 + 0.5 * sin(uv.x * 8.0 + iTime);
    vec3 low = vec3(0.025, 0.04, 0.06);
    vec3 high = vec3(0.08, 0.12, 0.15);
    color = vec4(mix(low, high, wave) * (0.8 + 0.2 * iParameter), 1.0);
}
