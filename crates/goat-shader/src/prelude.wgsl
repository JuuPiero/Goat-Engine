// GoatEngine WGSL prelude: what the standard material (goat-gfx-wgpu standard.wgsl) and every
// compiled Goat shader share — per-frame / per-draw uniforms, the material bindings, the standard
// vertex stage, shadows and the lighting model (per-pixel Lambert + Blinn-Phong with up to 8
// lights, hemisphere ambient, linear fog: the same model the 3DS draws with its fragment
// lighting). Compiled shaders use the `goat_*` functions and `goat_params` / `goat_tex*`.
diagnostic(off, derivative_uniformity);

struct Light {
    position_range: vec4<f32>,   // xyz world position, w range
    direction_kind: vec4<f32>,   // xyz direction the light travels, w kind (0 dir, 1 point, 2 spot)
    color: vec4<f32>,
    spot: vec4<f32>,             // x cos inner, y cos outer
};

struct Frame {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,       // w = light count
    camera_forward: vec4<f32>,   // w = 1: vertex colors are sRGB
    ambient: vec4<f32>,
    ambient_ground: vec4<f32>,   // hemisphere ambient from below, w = 1 when on
    fog_color: vec4<f32>,        // w = 1 when fog is on
    fog_range: vec4<f32>,        // x start, y end, z time (seconds)
    lights: array<Light, 8>,
    light_view_proj: mat4x4<f32>,
    shadow: vec4<f32>,           // x = shadowed light + 1 (0 = none), y = bias, z = 1 / map size
};

struct Object {
    model: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
    base_color: vec4<f32>,
    specular: vec4<f32>,    // rgb, w = shininess
    emissive: vec4<f32>,
    flags: vec4<f32>,       // x = lit (1/0), y = fogged (1/0), z = reflectivity, w = alpha cutoff (< 0: dithered)
    toon: vec4<f32>,        // x = steps (0 = smooth), y = rim power, z = outline width
    rim: vec4<f32>,
    outline: vec4<f32>,
    procedural: vec4<f32>,  // x = kind + 1 (1 water, 2 fire, 3 noise; 0 none), y = scale, z = speed, w = time
    proc_a: vec4<f32>,
    proc_b: vec4<f32>,
    probe_ambient: vec4<f32>,  // light probes: ambient, w = 1 when the object has probe light
    probe_dir: vec4<f32>,      // xyz the direction the probe light travels
    probe_color: vec4<f32>,
};

/// A compiled shader's material properties (colors, vectors, floats, texture tiling / offset).
struct GoatParams {
    v: array<vec4<f32>, 16>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var<uniform> object: Object;
@group(2) @binding(0) var goat_tex0: texture_2d<f32>;
@group(2) @binding(1) var goat_sampler: sampler;
@group(2) @binding(2) var goat_tex1: texture_2d<f32>;
@group(2) @binding(3) var env_cube: texture_cube<f32>;
@group(2) @binding(4) var env_sampler: sampler;
@group(2) @binding(5) var<uniform> goat_params: GoatParams;
@group(2) @binding(6) var goat_tex2: texture_2d<f32>;
@group(2) @binding(7) var goat_tex3: texture_2d<f32>;
@group(3) @binding(0) var shadow_map: texture_depth_2d;
@group(3) @binding(1) var shadow_sampler: sampler_comparison;

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

/// Vertex colors are authored in sRGB like every engine color.
fn goat_vertex_color(c: vec4<f32>) -> vec4<f32> {
    return select(c, vec4<f32>(pow(c.rgb, vec3<f32>(2.2)), c.a), frame.camera_forward.w > 0.5);
}

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    let world = object.model * vec4<f32>(v.position, 1.0);
    o.world_pos = world.xyz;
    o.clip = frame.view_proj * world;
    o.normal = (object.normal_matrix * vec4<f32>(v.normal, 0.0)).xyz;
    o.uv = v.uv;
    o.color = goat_vertex_color(v.color);
    return o;
}

@vertex
fn vs_shadow(v: VsIn) -> @builtin(position) vec4<f32> {
    return frame.light_view_proj * (object.model * vec4<f32>(v.position, 1.0));
}

/// Seconds since the game started.
fn goat_time() -> f32 {
    return frame.fog_range.z;
}

/// 0 = in shadow, 1 = lit (3×3 PCF over the shadow map).
fn goat_shadow(world_pos: vec3<f32>) -> f32 {
    let p = frame.light_view_proj * vec4<f32>(world_pos, 1.0);
    let ndc = p.xyz / p.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || ndc.z > 1.0) {
        return 1.0;
    }
    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2<f32>(f32(x), f32(y)) * frame.shadow.z;
            sum += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + o, ndc.z - frame.shadow.y);
        }
    }
    return sum / 9.0;
}

fn goat_attenuation(d: f32, range: f32) -> f32 {
    let x = min(d / max(range, 1e-4), 1.0);
    let w = 1.0 - x * x;
    return w * w;
}

/// The engine's lighting of a surface: albedo × (ambient + Σ diffuse) + specular color × Σ
/// Blinn-Phong highlights (`shininess` 1..256); `toon_steps` > 0 bands the diffuse light.
fn goat_light(world_pos: vec3<f32>, normal: vec3<f32>, albedo: vec3<f32>, specular_color: vec3<f32>, shininess: f32, toon_steps: f32) -> vec3<f32> {
    let n = normalize(normal);
    let v = normalize(frame.camera_pos.xyz - world_pos);
    var diffuse = frame.ambient.rgb;
    if (frame.ambient_ground.w > 0.5) {
        // hemisphere: ground color from below to sky (ambient) from above
        diffuse = mix(frame.ambient_ground.rgb, frame.ambient.rgb, 0.5 + 0.5 * n.y);
    }
    if (object.probe_ambient.w > 0.5) {
        // light probes: their ambient + one directional light replace the view's ambient
        diffuse = object.probe_ambient.rgb + object.probe_color.rgb * max(dot(n, -object.probe_dir.xyz), 0.0);
    }
    var specular = vec3<f32>(0.0);
    let count = u32(frame.camera_pos.w);
    for (var k = 0u; k < count; k++) {
        let light = frame.lights[k];
        let kind = light.direction_kind.w;
        var l = -light.direction_kind.xyz;
        var strength = 1.0;
        if (kind > 0.5) {
            let to_light = light.position_range.xyz - world_pos;
            let d = length(to_light);
            l = to_light / max(d, 1e-4);
            strength = goat_attenuation(d, light.position_range.w);
            if (kind > 1.5) {
                let cos_angle = dot(-l, light.direction_kind.xyz);
                strength *= smoothstep(light.spot.y, light.spot.x, cos_angle);
            }
        }
        var ndl = max(dot(n, l), 0.0);
        if (f32(k) + 1.0 == frame.shadow.x) {
            strength *= goat_shadow(world_pos);
        }
        let h = normalize(l + v);
        var spec = select(0.0, pow(max(dot(n, h), 0.0), shininess), ndl > 0.0);
        if (toon_steps > 0.5) {
            // cel shading: flat bands, a hard highlight
            ndl = round(ndl * toon_steps) / toon_steps;
            spec = step(0.5, spec);
        }
        diffuse += light.color.rgb * ndl * strength;
        specular += light.color.rgb * spec * strength;
    }
    return albedo * diffuse + specular_color * specular;
}

/// The view's linear fog over a color (when the material is fogged).
fn goat_fog(world_pos: vec3<f32>, rgb: vec3<f32>) -> vec3<f32> {
    if (frame.fog_color.w > 0.5 && object.flags.y > 0.5) {
        let depth = dot(world_pos - frame.camera_pos.xyz, frame.camera_forward.xyz);
        let visibility = clamp((frame.fog_range.y - depth) / (frame.fog_range.y - frame.fog_range.x), 0.0, 1.0);
        return mix(frame.fog_color.rgb, rgb, visibility);
    }
    return rgb;
}
