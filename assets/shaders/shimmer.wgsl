// Rainbow shimmer overlay — full-screen triangle, animated band in fragment shader.

struct ShimmerParams {
    elapsed_s: f32,
    duration_s: f32,
    opacity: f32,
    speed: f32,
}

@group(0) @binding(0)
var<uniform> params: ShimmerParams;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    // Full-screen triangle (covers entire NDC).
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let pos = positions[vertex_index];
    var out: VertexOutput;
    out.position = vec4<f32>(pos, 0.0, 1.0);
    out.uv = pos * 0.5 + 0.5;
    return out;
}

fn hue_to_rgb(h: f32) -> vec3<f32> {
    let k = vec3<f32>(0.0, 2.0 / 3.0, 1.0 / 3.0);
    let p = abs(fract(vec3<f32>(h) + k) * 6.0 - 3.0);
    return clamp(p - 1.0, vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let progress = clamp(params.elapsed_s / max(params.duration_s, 0.001), 0.0, 1.0);
    let fade_in = smoothstep(0.0, 0.08, progress);
    let fade_out = 1.0 - smoothstep(0.88, 1.0, progress);
    let envelope = fade_in * fade_out;

    let phase = params.elapsed_s * params.speed * 0.35;
    let band_center = fract(phase + in.uv.x * 0.25 + in.uv.y * 0.1);
    let band = smoothstep(0.42, 0.0, abs(band_center - 0.5) * 2.0);

    let hue = fract(in.uv.x * 0.85 + in.uv.y * 0.35 + phase * 0.15);
    let rgb = hue_to_rgb(hue);

    let shimmer = band * envelope * params.opacity;
    return vec4<f32>(rgb * shimmer, shimmer);
}
