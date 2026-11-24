// GoatEngine standard material (desktop): texture × vertex color × base color, per-pixel
// Lambert + Blinn-Phong with up to 8 lights (directional / point / spot) + ambient and linear
// fog — the same model the 3DS draws with its fragment lighting and fog unit. Attenuation and
// spot cones use goat_gfx::Light::attenuation / spot_factor.

struct Light {
    position_range: vec4<f32>,   // xyz world position, w range
    direction_kind: vec4<f32>,   // xyz direction the light travels, w kind (0 dir, 1 point, 2 spot)
    color: vec4<f32>,
    spot: vec4<f32>,             // x cos inner, y cos outer
};

struct Frame {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,       // w = light count
    camera_forward: vec4<f32>,
    ambient: vec4<f32>,
    ambient_ground: vec4<f32>,   // hemisphere ambient from below, w = 1 when on
    fog_color: vec4<f32>,        // w = 1 when fog is on
    fog_range: vec4<f32>,        // x start, y end
    lights: array<Light, 8>,
};

struct Object {
    model: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
    base_color: vec4<f32>,
    specular: vec4<f32>,    // rgb, w = shininess
    emissive: vec4<f32>,
    flags: vec4<f32>,       // x = lit (1/0), y = fogged (1/0), z = reflectivity, w = alpha cutoff (< 0: dithered)
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var<uniform> object: Object;
@group(2) @binding(0) var base_tex: texture_2d<f32>;
@group(2) @binding(1) var base_sampler: sampler;
@group(2) @binding(2) var emissive_tex: texture_2d<f32>;
@group(2) @binding(3) var env_cube: texture_cube<f32>;
@group(2) @binding(4) var env_sampler: sampler;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    let world = object.model * vec4<f32>(v.position, 1.0);
    o.world_pos = world.xyz;
    o.clip = frame.view_proj * world;
    o.normal = (object.normal_matrix * vec4<f32>(v.normal, 0.0)).xyz;
    o.uv = v.uv;
    // vertex colors are authored in sRGB like every engine color
    o.color = select(v.color, vec4<f32>(pow(v.color.rgb, vec3<f32>(2.2)), v.color.a), frame.camera_forward.w > 0.5);
    return o;
}

fn attenuation(d: f32, range: f32) -> f32 {
    let x = min(d / max(range, 1e-4), 1.0);
    let w = 1.0 - x * x;
    return w * w;
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
    let albedo = textureSample(base_tex, base_sampler, i.uv) * i.color * object.base_color;
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
        let n = normalize(i.normal);
        let v = normalize(frame.camera_pos.xyz - i.world_pos);
        var diffuse = frame.ambient.rgb;
        if (frame.ambient_ground.w > 0.5) {
            // hemisphere: ground color from below to sky (ambient) from above
            diffuse = mix(frame.ambient_ground.rgb, frame.ambient.rgb, 0.5 + 0.5 * n.y);
        }
        var specular = vec3<f32>(0.0);
        let count = u32(frame.camera_pos.w);
        for (var k = 0u; k < count; k++) {
            let light = frame.lights[k];
            let kind = light.direction_kind.w;
            var l = -light.direction_kind.xyz;
            var strength = 1.0;
            if (kind > 0.5) {
                let to_light = light.position_range.xyz - i.world_pos;
                let d = length(to_light);
                l = to_light / max(d, 1e-4);
                strength = attenuation(d, light.position_range.w);
                if (kind > 1.5) {
                    let cos_angle = dot(-l, light.direction_kind.xyz);
                    strength *= smoothstep(light.spot.y, light.spot.x, cos_angle);
                }
            }
            let ndl = max(dot(n, l), 0.0);
            let h = normalize(l + v);
            let spec = select(0.0, pow(max(dot(n, h), 0.0), object.specular.w), ndl > 0.0);
            diffuse += light.color.rgb * ndl * strength;
            specular += light.color.rgb * spec * strength;
        }
        rgb = albedo.rgb * diffuse + object.specular.rgb * specular;
    }
    // cube map reflection (sampled always: uniform control flow)
    let nr = normalize(i.normal);
    let r = reflect(-normalize(frame.camera_pos.xyz - i.world_pos), nr);
    let env = textureSample(env_cube, env_sampler, r).rgb;
    rgb = mix(rgb, env, object.flags.z);
    rgb = rgb + object.emissive.rgb * textureSample(emissive_tex, base_sampler, i.uv).rgb;
    if (frame.fog_color.w > 0.5 && object.flags.y > 0.5) {
        let depth = dot(i.world_pos - frame.camera_pos.xyz, frame.camera_forward.xyz);
        let visibility = clamp((frame.fog_range.y - depth) / (frame.fog_range.y - frame.fog_range.x), 0.0, 1.0);
        rgb = mix(frame.fog_color.rgb, rgb, visibility);
    }
    return vec4<f32>(rgb, albedo.a);
}
