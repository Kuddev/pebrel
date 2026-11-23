struct Parameters { value: vec4<f32> }
@group(0) @binding(0) var<uniform> parameters: Parameters;
@fragment
fn main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = (position.xy - 0.5 * parameters.value.xy) / parameters.value.y;
    let t = 6.28318530718 * parameters.value.z / 6.0;
    var f = sin(4.0 * p.x + 1.5 * sin(3.0 * p.y + t)) + cos(5.0 * p.y + 1.2 * cos(3.0 * p.x - t));
    f += 0.7 * sin(12.0 * length(p - vec2<f32>(0.3 * cos(t), 0.18 * sin(t))) - t);
    var color = vec3<f32>(0.5) + 0.5 * cos(vec3<f32>(0.1, 2.05, 4.1) + vec3<f32>(f * 1.7 + t) + vec3<f32>(p.x, p.y, -p.x));
    let ridges = pow(0.5 + 0.5 * cos(f * 13.0 + t), 18.0);
    color = mix(vec3<f32>(0.018, 0.012, 0.055), color, 0.68) + vec3<f32>(0.25, 0.4, 0.65) * ridges * 0.35;
    color *= 0.85 + 0.15 * cos(length(p) * 3.0);
    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
