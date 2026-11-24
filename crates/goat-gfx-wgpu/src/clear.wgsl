// Clears a viewport's color and depth (wgpu clears whole attachments only).
struct ClearOut {
    @builtin(position) clip: vec4<f32>,
};

@group(0) @binding(0) var<uniform> clear_color: vec4<f32>;

@vertex
fn vs_clear(@builtin(vertex_index) i: u32) -> ClearOut {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var o: ClearOut;
    o.clip = vec4<f32>(p[i], 1.0, 1.0);
    return o;
}

@fragment
fn fs_clear() -> @location(0) vec4<f32> {
    return clear_color;
}
