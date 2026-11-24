// GoatEngine standard material (desktop), after goat-shader's prelude (uniforms, bindings,
// vs_main, shadows, goat_light, goat_fog): texture × vertex color × base color, per-pixel
// Lambert + Blinn-Phong with up to 8 lights (directional / point / spot) + ambient and linear
// fog — the same model the 3DS draws with its fragment lighting and fog unit. Attenuation and
// spot cones use goat_gfx::Light::attenuation / spot_factor.

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

/// The procedural texture (rgba) at uv: same looks as the 3DS proctex setups.
fn procedural(uv: vec2<f32>) -> vec4<f32> {
    let kind = object.procedural.x;
    let s = object.procedural.y;
    let t = object.procedural.w * object.procedural.z;
    let n = vnoise(uv * s + vec2<f32>(t * 0.6, t * 0.4)) * 0.6 + vnoise(uv * s * 2.0 - vec2<f32>(t * 0.3, t)) * 0.4;
    if (kind < 1.5) {
        // water: mirrored bands of (u + v) bent by the noise
        let x = fract((uv.x + uv.y) * 2.0 + n * 0.8 + t * 0.15) * 2.0;
        let band = select(x, 2.0 - x, x > 1.0);
        return vec4<f32>(mix(object.proc_a.rgb, object.proc_b.rgb, band), 1.0);
    }
    if (kind < 2.5) {
        // fire: strong at the base (v = 1), torn by the noise, fading up
        let h = clamp((1.0 - uv.y) * 1.2 + (n - 0.5) * 0.8, 0.0, 1.0);
        let c = mix(object.proc_a.rgb, object.proc_b.rgb, h);
        return vec4<f32>(c, clamp(1.0 - h * h, 0.0, 1.0));
    }
    return vec4<f32>(mix(object.proc_a.rgb, object.proc_b.rgb, n), 1.0);
}

// 4×4 Bayer thresholds (0..1)
fn bayer(p: vec2<f32>) -> f32 {
    let m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    let x = u32(p.x) % 4u;
    let y = u32(p.y) % 4u;
    return (m[y * 4u + x] + 0.5) / 16.0;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
    var tex = textureSample(goat_tex0, goat_sampler, i.uv);
    if (object.procedural.x > 0.5) {
        tex = procedural(i.uv);
    }
    let albedo = tex * i.color * object.base_color;
    // alpha test (dithered: the cutoff spreads over ±0.25 with a screen pattern)
    let cutoff = abs(object.flags.w);
    if (cutoff > 0.0) {
        var c = cutoff;
        if (object.flags.w < 0.0) {
            c = cutoff + (bayer(i.clip.xy) - 0.5) * 0.5;
        }
        if (albedo.a < c) {
            discard;
        }
    }
    var rgb = albedo.rgb;
    if (object.flags.x > 0.5) {
        rgb = goat_light(i.world_pos, i.normal, albedo.rgb, object.specular.rgb, object.specular.w, object.toon.x);
    }
    // rim light (fresnel)
    let vr = normalize(frame.camera_pos.xyz - i.world_pos);
    rgb = rgb + object.rim.rgb * pow(1.0 - max(dot(normalize(i.normal), vr), 0.0), object.toon.y);
    // cube map reflection (sampled always: uniform control flow)
    let nr = normalize(i.normal);
    let r = reflect(-normalize(frame.camera_pos.xyz - i.world_pos), nr);
    let env = textureSample(env_cube, env_sampler, r).rgb;
    rgb = mix(rgb, env, object.flags.z);
    rgb = rgb + object.emissive.rgb * textureSample(goat_tex1, goat_sampler, i.uv).rgb;
    rgb = goat_fog(i.world_pos, rgb);
    return vec4<f32>(rgb, albedo.a);
}

// Outline pass: back faces pushed out along the normals, one flat color.
@vertex
fn vs_outline(v: VsIn) -> VsOut {
    var o: VsOut;
    let n = normalize((object.normal_matrix * vec4<f32>(v.normal, 0.0)).xyz);
    let world = object.model * vec4<f32>(v.position, 1.0) + vec4<f32>(n * object.toon.z, 0.0);
    o.world_pos = world.xyz;
    o.clip = frame.view_proj * world;
    o.normal = n;
    o.uv = v.uv;
    o.color = v.color;
    return o;
}

@fragment
fn fs_outline(i: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(object.outline.rgb, 1.0);
}
