struct PebrelFrame {
    viewport: vec4<f32>,
    time: vec4<f32>,
    flags: vec4<u32>,
    cursor: vec4<f32>,
    previous_cursor: vec4<f32>,
    cursor_color: vec4<f32>,
    previous_cursor_color: vec4<f32>,
    cursor_styles: vec4<u32>,
    foreground: vec4<f32>,
    background: vec4<f32>,
    cursor_text: vec4<f32>,
    selection_foreground: vec4<f32>,
    selection_background: vec4<f32>,
    palette: array<vec4<f32>, 256>,
}
@group(0) @binding(0) var<uniform> frame: PebrelFrame;
@group(0) @binding(1) var surface: texture_2d<f32>;

fn load_surface(pixel: vec2<i32>) -> vec4<f32> {
    let last = vec2<i32>(textureDimensions(surface)) - vec2<i32>(1);
    return textureLoad(surface, clamp(pixel, vec2<i32>(0), last), 0);
}

// 固定单层终端纹理采用 clamp-to-edge 双线性采样；不依赖后端的绑定堆接口。
fn sample_surface(uv: vec2<f32>) -> vec4<f32> {
    let at = uv * vec2<f32>(textureDimensions(surface)) - vec2<f32>(0.5);
    let origin = vec2<i32>(floor(at));
    let weight = fract(at);
    let a = mix(load_surface(origin), load_surface(origin + vec2<i32>(1, 0)), weight.x);
    let b = mix(load_surface(origin + vec2<i32>(0, 1)), load_surface(origin + vec2<i32>(1, 1)), weight.x);
    return mix(a, b, weight.y);
}
