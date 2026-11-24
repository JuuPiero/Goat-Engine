// GoatEngine post effects (desktop): bright pass → blur → composite with grading and vignette.

struct Post {
    grade: vec4<f32>,   // x exposure, y contrast, z saturation, w vignette
    bloom: vec4<f32>,   // x strength, y threshold, zw texel size of the source
    tint: vec4<f32>,
    dir: vec4<f32>,     // xy blur direction (texels)
};

@group(0) @binding(0) var src_a: texture_2d<f32>;
@group(0) @binding(1) var src_b: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<uniform> post: Post;

struct V {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// one triangle covering the screen
@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> V {
    var o: V;
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fs_bright(i: V) -> @location(0) vec4<f32> {
    // 4 taps (a 2×2 box per output texel of the quarter-size target)
    let t = post.bloom.zw;
    var c = vec3<f32>(0.0);
    c += textureSample(src_a, samp, i.uv + vec2<f32>(-t.x, -t.y)).rgb;
    c += textureSample(src_a, samp, i.uv + vec2<f32>(t.x, -t.y)).rgb;
    c += textureSample(src_a, samp, i.uv + vec2<f32>(-t.x, t.y)).rgb;
    c += textureSample(src_a, samp, i.uv + vec2<f32>(t.x, t.y)).rgb;
    c = c * 0.25 * post.grade.x;
    let k = max(luma(c) - post.bloom.y, 0.0) / max(luma(c), 1e-4);
    return vec4<f32>(c * k, 1.0);
}

@fragment
fn fs_blur(i: V) -> @location(0) vec4<f32> {
    let w = array<f32, 5>(0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216);
    var c = textureSample(src_a, samp, i.uv).rgb * w[0];
    for (var k = 1; k < 5; k++) {
        let o = post.dir.xy * f32(k);
        c += textureSample(src_a, samp, i.uv + o).rgb * w[k];
        c += textureSample(src_a, samp, i.uv - o).rgb * w[k];
    }
    return vec4<f32>(c, 1.0);
}

@fragment
fn fs_composite(i: V) -> @location(0) vec4<f32> {
    var c = textureSample(src_a, samp, i.uv).rgb * post.grade.x;
    c += textureSample(src_b, samp, i.uv).rgb * post.bloom.x;
    c *= post.tint.rgb;
    // contrast around mid gray, saturation around the luminance
    c = (c - 0.5) * post.grade.y + 0.5;
    let l = luma(c);
    c = mix(vec3<f32>(l), c, post.grade.z);
    // vignette
    let d = distance(i.uv, vec2<f32>(0.5));
    c *= 1.0 - post.grade.w * smoothstep(0.35, 0.85, d);
    return vec4<f32>(max(c, vec3<f32>(0.0)), 1.0);
}
