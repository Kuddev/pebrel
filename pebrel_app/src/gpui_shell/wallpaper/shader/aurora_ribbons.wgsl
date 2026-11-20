struct Parameters { value: vec4<f32> }
@group(0) @binding(0) var<uniform> parameters: Parameters;
@fragment
fn main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = position.xy / parameters.value.xy;
    let t = 6.28318530718 * parameters.value.z / 6.0;
    var color = mix(vec3<f32>(0.005, 0.01, 0.035), vec3<f32>(0.025, 0.025, 0.12), uv.y);
    for (var i = 0; i < 4; i += 1) {
        let fi = f32(i);
        let y = 0.35 + 0.09 * fi + 0.09 * sin(uv.x * 6.0 + t + fi) + 0.055 * sin(uv.x * 13.0 - t + fi * 1.7);
        let d = uv.y - y;
        let curtain = exp(-abs(d) * 22.0) * (0.55 + 0.45 * sin(uv.x * 65.0 + sin(uv.x * 12.0 + t) * 4.0 + fi));
        let hue = mix(vec3<f32>(0.02, 0.85, 0.62), vec3<f32>(0.38, 0.16, 1.0), fi / 3.0);
        color += hue * curtain * 0.38 + hue * exp(-abs(d) * 115.0) * 0.45;
    }
    let cell = floor(uv * vec2<f32>(110.0, 64.0));
    let h = fract(sin(dot(cell, vec2<f32>(127.1, 311.7))) * 43758.5453);
    let local = fract(uv * vec2<f32>(110.0, 64.0)) - vec2<f32>(0.5);
    let star = (1.0 - smoothstep(0.0, 0.10, length(local))) * step(0.989, h) * (0.65 + 0.35 * sin(t + h * 20.0));
    color += vec3<f32>(0.65, 0.8, 1.0) * star;
    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
