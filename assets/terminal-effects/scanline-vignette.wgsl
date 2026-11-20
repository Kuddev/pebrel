@fragment
fn scanlines(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {
    let original = sample_surface(pixel.xy / frame.viewport.xy);
    let band = 0.96 + 0.04 * sin(pixel.y * 3.14159265);
    return vec4<f32>(original.rgb * band, original.a);
}

@fragment
fn edge_shading(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {
    let original = load_surface(vec2<i32>(pixel.xy));
    let offset = pixel.xy / frame.viewport.xy * 2.0 - vec2<f32>(1.0);
    let shade = 1.0 - 0.10 * clamp(dot(offset, offset), 0.0, 1.0);
    return vec4<f32>(original.rgb * shade, original.a);
}
