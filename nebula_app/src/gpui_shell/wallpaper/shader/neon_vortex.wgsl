struct Parameters { value: vec4<f32> }
@group(0) @binding(0) var<uniform> parameters: Parameters;
@fragment
fn main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = (position.xy - 0.5 * parameters.value.xy) / parameters.value.y;
    let t = 6.28318530718 * parameters.value.z / 6.0;
    let r = length(p);
    let a = atan2(p.y, p.x);
    let radial_offset = 0.08 * sin(3.0 * a + t) + 0.035 * cos(7.0 * a - t);
    let ribbons = pow(0.5 + 0.5 * sin(26.0 * (r + radial_offset) - 2.0 * t + 3.0 * a), 12.0);
    let inner = exp(-28.0 * abs(r - 0.23 - 0.025 * sin(4.0 * a - t)));
    let light = mix(vec3<f32>(0.66, 0.09, 1.0), vec3<f32>(0.03, 0.82, 1.0), 0.5 + 0.5 * sin(2.0 * a + t + 5.0 * r));
    var color = vec3<f32>(0.012, 0.016, 0.055) + light * (ribbons * 0.85 + inner * 0.7) * exp(-1.3 * r);
    color += vec3<f32>(0.95, 0.1, 0.5) * pow(0.5 + 0.5 * cos(45.0 * r + sin(a + t)), 30.0) * 0.18;
    color *= 0.55 + 0.45 * smoothstep(0.0, 0.12, r);
    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
