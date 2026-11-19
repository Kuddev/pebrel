layout(std140, set = 0, binding = 0) uniform PebrelFrame {
    vec4 viewport;
    vec4 time;
    uvec4 flags;
    vec4 cursor;
    vec4 previous_cursor;
    vec4 cursor_color;
    vec4 previous_cursor_color;
    uvec4 cursor_styles;
    vec4 foreground;
    vec4 background;
    vec4 cursor_text;
    vec4 selection_foreground;
    vec4 selection_background;
    vec4 palette[256];
    vec4 parameters;
} frame;
layout(set = 0, binding = 1) uniform texture2D surface;

#define iResolution vec3(frame.viewport.xy, 1.0)
#define iTime frame.time.x
#define iParameters frame.parameters

vec4 load_surface(ivec2 pixel) {
    ivec2 last = textureSize(surface, 0) - ivec2(1);
    return texelFetch(surface, clamp(pixel, ivec2(0), last), 0);
}

vec4 sample_surface(vec2 uv) {
    vec2 at = uv * vec2(textureSize(surface, 0)) - vec2(0.5);
    ivec2 origin = ivec2(floor(at));
    vec2 weight = fract(at);
    vec4 a = mix(load_surface(origin), load_surface(origin + ivec2(1, 0)), weight.x);
    vec4 b = mix(load_surface(origin + ivec2(0, 1)), load_surface(origin + ivec2(1, 1)), weight.x);
    return mix(a, b, weight.y);
}
