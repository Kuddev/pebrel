// Terminal GLSL can sample the preceding pass and read four configured scalars.
void mainImage(out vec4 color, in vec2 pixel) {
    color = sample_surface(pixel / iResolution.xy);
    float strength = clamp(iParameters.x, 0.0, 1.0);
    float wave = 0.5 + 0.5 * sin(pixel.y * 3.14159265 + iTime);
    color.rgb *= 1.0 - strength * 0.08 * wave;
}
