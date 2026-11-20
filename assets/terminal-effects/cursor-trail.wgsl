@fragment
fn cursor_trail(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {
    let original = load_surface(vec2<i32>(pixel.xy));
    if frame.flags.z == 0u || frame.flags.w == 0u {
        return original;
    }
    let current = frame.cursor.xy + frame.cursor.zw * 0.5;
    let previous = frame.previous_cursor.xy + frame.previous_cursor.zw * 0.5;
    let segment = current - previous;
    let projected = clamp(dot(pixel.xy - previous, segment) / max(dot(segment, segment), 0.001), 0.0, 1.0);
    let distance = length(pixel.xy - mix(previous, current, projected));
    let age = max(frame.time.x - frame.time.z, 0.0);
    let glow = exp(-distance / max(frame.cursor.w * 0.35, 1.0)) * exp(-age * 9.0) * 0.35;
    return vec4<f32>(mix(original.rgb, frame.cursor_color.rgb * original.a, glow), original.a);
}
