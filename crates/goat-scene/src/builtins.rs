//! Codecs of the engine's built-in components (field names in scene files).

use goat_assets::material::FileColor;
use goat_core::math::{EulerRot, Quat, Vec3};
use goat_core::Color;
use goat_gfx::{Display, Fog, Projection, Viewport};
use goat_runtime::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::registry::{remove_typed, ComponentCodec, LoadContext, SaveContext, SceneRegistry};

fn to_value<T: Serialize>(v: T) -> Option<Value> {
    serde_json::to_value(v).ok()
}
fn from_value<T: for<'de> Deserialize<'de>>(v: &Value) -> Result<T, String> {
    serde_json::from_value(v.clone()).map_err(|e| e.to_string())
}
fn insert<C: hecs::Component>(ctx: &mut LoadContext, e: Entity, c: C) -> Result<(), String> {
    ctx.world.insert_one(e, c).map_err(|_| "entity vanished".to_string())
}

// ------------------------------------------------------------------ Transform

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct TransformJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<[f32; 3]>,
    /// Quaternion x y z w.
    #[serde(skip_serializing_if = "Option::is_none")]
    rotation: Option<[f32; 4]>,
    /// Alternative when hand-writing scenes: Euler angles in degrees (Unity's order: Z, X, Y).
    #[serde(skip_serializing_if = "Option::is_none")]
    euler: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scale: Option<[f32; 3]>,
}

fn save_transform(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let t = *ctx.world.get::<&Transform>(e).ok()?;
    let d = Transform::default();
    to_value(TransformJson {
        position: (t.position != d.position).then(|| t.position.into()),
        rotation: (t.rotation != d.rotation).then(|| t.rotation.to_array()),
        euler: None,
        scale: (t.scale != d.scale).then(|| t.scale.into()),
    })
}

fn load_transform(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: TransformJson = from_value(v)?;
    let mut t = Transform::default();
    if let Some(p) = j.position {
        t.position = Vec3::from(p);
    }
    if let Some(r) = j.rotation {
        let q = Quat::from_array(r);
        // normalizing an already unit quaternion changes its last bits: saves would drift
        t.rotation = if (q.length() - 1.0).abs() > 1e-5 { q.normalize() } else { q };
    } else if let Some([x, y, z]) = j.euler {
        t.rotation = Quat::from_euler(EulerRot::YXZ, y.to_radians(), x.to_radians(), z.to_radians());
    }
    if let Some(s) = j.scale {
        t.scale = Vec3::from(s);
    }
    insert(ctx, e, t)
}

// ------------------------------------------------------------------ Camera

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProjectionJson {
    Perspective { fov: f32, near: f32, far: f32 },
    Orthographic { height: f32, near: f32, far: f32 },
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct CameraJson {
    display: String,
    viewport: [f32; 4],
    projection: ProjectionJson,
    clear: Option<FileColor>,
    clear_depth: bool,
    depth: i32,
    culling_mask: u32,
    /// A `.renderTexture.json` key (Unity's Target Texture); none / "" = the display.
    #[serde(skip_serializing_if = "Option::is_none")]
    target_texture: Option<String>,
}

impl Default for CameraJson {
    fn default() -> Self {
        let c = Camera::default();
        CameraJson {
            display: "main".into(),
            viewport: [0.0, 0.0, 1.0, 1.0],
            projection: ProjectionJson::Perspective { fov: 60.0, near: 0.1, far: 200.0 },
            clear: c.clear.map(FileColor),
            clear_depth: true,
            depth: 0,
            culling_mask: u32::MAX,
            target_texture: None,
        }
    }
}

fn save_camera(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let c = *ctx.world.get::<&Camera>(e).ok()?;
    to_value(CameraJson {
        display: if c.display == Display::Secondary { "secondary" } else { "main" }.into(),
        viewport: [c.viewport.x, c.viewport.y, c.viewport.width, c.viewport.height],
        projection: match c.projection {
            Projection::Perspective { fov_y_radians, near, far } => ProjectionJson::Perspective { fov: fov_y_radians.to_degrees(), near, far },
            Projection::Orthographic { height, near, far } => ProjectionJson::Orthographic { height, near, far },
        },
        clear: c.clear.map(FileColor),
        clear_depth: c.clear_depth,
        depth: c.depth,
        culling_mask: c.culling_mask,
        target_texture: c.target_texture.and_then(|t| ctx.assets.key_of_texture(t)).map(str::to_string),
    })
}

fn load_camera(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: CameraJson = from_value(v)?;
    let [x, y, w, h] = j.viewport;
    let target_texture = match j.target_texture.as_deref().filter(|k| !k.is_empty()) {
        Some(k) => Some(ctx.assets.texture(ctx.gfx, k).map_err(|e| e.to_string())?),
        None => None,
    };
    insert(
        ctx,
        e,
        Camera {
            display: if j.display == "secondary" { Display::Secondary } else { Display::Main },
            viewport: Viewport { x, y, width: w, height: h },
            projection: match j.projection {
                ProjectionJson::Perspective { fov, near, far } => Projection::Perspective { fov_y_radians: fov.to_radians(), near, far },
                ProjectionJson::Orthographic { height, near, far } => Projection::Orthographic { height, near, far },
            },
            clear: j.clear.map(|c| c.0),
            clear_depth: j.clear_depth,
            depth: j.depth,
            culling_mask: j.culling_mask,
            target_texture,
        },
    )
}

// ------------------------------------------------------------------ MeshFilter + MeshRenderer

#[derive(Serialize, Deserialize)]
struct MeshFilterJson {
    mesh: String,
}

#[derive(Serialize, Deserialize)]
struct MeshRendererJson {
    #[serde(default)]
    materials: Option<Vec<String>>,
    /// Scenes saved before MeshFilter existed: the mesh lived on the renderer, with one material.
    #[serde(default, skip_serializing)]
    mesh: Option<String>,
    #[serde(default, skip_serializing)]
    material: Option<String>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    visible: bool,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    layer: u8,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    sorting_order: i32,
}

fn yes() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}
fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}
fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

fn save_mesh_filter(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let f = ctx.world.get::<&MeshFilter>(e).ok()?;
    let Some(mesh) = ctx.assets.key_of_submeshes(&f.submeshes) else {
        goat_core::warn!("scene save: a MeshFilter uses a mesh made in code (no asset key): skipped");
        return None;
    };
    to_value(MeshFilterJson { mesh })
}

fn load_mesh_filter(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: MeshFilterJson = from_value(v)?;
    let submeshes = ctx.assets.mesh_filter(ctx.gfx, &j.mesh).map_err(|e| e.to_string())?;
    insert(ctx, e, MeshFilter::with_submeshes(submeshes))
}

fn save_mesh_renderer(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let r = ctx.world.get::<&MeshRenderer>(e).ok()?;
    let mut materials = Vec::with_capacity(r.materials.len());
    for m in &r.materials {
        let Some(key) = ctx.assets.key_of_material(*m) else {
            goat_core::warn!("scene save: a MeshRenderer uses a material made in code (no asset key): skipped");
            return None;
        };
        materials.push(key.to_string());
    }
    to_value(MeshRendererJson { materials: Some(materials), mesh: None, material: None, visible: r.visible, layer: r.layer, sorting_order: r.sorting_order })
}

fn load_mesh_renderer(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: MeshRendererJson = from_value(v)?;
    let keys = match j.materials {
        Some(list) => list,
        None => vec![j.material.unwrap_or_else(|| "builtin:default".into())],
    };
    let mut materials = Vec::with_capacity(keys.len());
    for k in &keys {
        materials.push(ctx.assets.material(ctx.gfx, k).map_err(|e| e.to_string())?);
    }
    // the old format: the renderer's mesh becomes the object's MeshFilter
    if let Some(mesh) = &j.mesh {
        if ctx.world.get::<&MeshFilter>(e).is_err() {
            let submeshes = ctx.assets.mesh_filter(ctx.gfx, mesh).map_err(|e| e.to_string())?;
            insert(ctx, e, MeshFilter::with_submeshes(submeshes))?;
        }
    }
    let r = MeshRenderer { visible: j.visible, ..MeshRenderer::with_materials(materials).with_layer(j.layer).with_sorting_order(j.sorting_order) };
    insert(ctx, e, r)
}

// ------------------------------------------------------------------ SkinnedMeshRenderer

#[derive(Serialize, Deserialize)]
struct SkinnedMeshRendererJson {
    mesh: String,
    #[serde(default)]
    materials: Option<Vec<String>>,
    /// Shown in the Inspector (the bones come from the model instance; not read back).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    root_bone: Option<String>,
    #[serde(default)]
    bones: usize,
    #[serde(default)]
    update_when_offscreen: bool,
    #[serde(default = "auto_quality")]
    quality: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    visible: bool,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    layer: u8,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    sorting_order: i32,
}

fn auto_quality() -> String {
    "Auto".into()
}

fn save_skinned_mesh_renderer(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let r = ctx.world.get::<&SkinnedMeshRenderer>(e).ok()?;
    let Some(mesh) = ctx.assets.key_of_submeshes(&r.shared) else {
        goat_core::warn!("scene save: a SkinnedMeshRenderer uses a mesh made in code (no asset key): skipped");
        return None;
    };
    let mut materials = Vec::with_capacity(r.materials.len());
    for m in &r.materials {
        materials.push(ctx.assets.key_of_material(*m)?.to_string());
    }
    let root_bone = r.root_bone.and_then(|b| ctx.world.get::<&Name>(b).ok().map(|n| n.0.clone()));
    to_value(SkinnedMeshRendererJson {
        mesh,
        materials: Some(materials),
        root_bone,
        bones: r.bones.len(),
        update_when_offscreen: r.update_when_offscreen,
        quality: r.quality.name().into(),
        visible: r.visible,
        layer: r.layer,
        sorting_order: r.sorting_order,
    })
}

fn load_skinned_mesh_renderer(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: SkinnedMeshRendererJson = from_value(v)?;
    let mut materials = Vec::new();
    for k in j.materials.unwrap_or_else(|| vec!["builtin:default".into()]) {
        materials.push(ctx.assets.material(ctx.gfx, &k).map_err(|e| e.to_string())?);
    }
    let shared = ctx.assets.mesh_filter(ctx.gfx, &j.mesh).map_err(|e| e.to_string())?;
    // a model instance's renderer (overrides): keep its skin and bones, take the settings
    if let Ok(mut r) = ctx.world.get::<&mut SkinnedMeshRenderer>(e) {
        if r.shared != shared {
            r.shared = shared;
            r.skin = None;
            r.deformed.clear();
        }
        r.materials = materials;
        r.update_when_offscreen = j.update_when_offscreen;
        r.quality = SkinQuality::from_name(&j.quality);
        r.visible = j.visible;
        r.layer = j.layer.min(31);
        r.sorting_order = j.sorting_order;
        return Ok(());
    }
    let mut r = SkinnedMeshRenderer::new(shared, materials);
    r.update_when_offscreen = j.update_when_offscreen;
    r.quality = SkinQuality::from_name(&j.quality);
    r.visible = j.visible;
    r.layer = j.layer.min(31);
    r.sorting_order = j.sorting_order;
    insert(ctx, e, r)
}

/// `"LodGroup": { "levels": [ { "mesh": "models/rock.glb#mesh:0:0", "screen_height": 0.4 }, ... ], "cull": true }`
#[derive(Serialize, Deserialize)]
struct LodJson {
    levels: Vec<LodLevelJson>,
    #[serde(default)]
    cull: bool,
}
#[derive(Serialize, Deserialize)]
struct LodLevelJson {
    mesh: String,
    screen_height: f32,
}

fn save_lod(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let l = ctx.world.get::<&goat_runtime::LodGroup>(e).ok()?;
    let levels = l.levels.iter().filter_map(|x| ctx.assets.key_of_mesh(x.mesh).map(|k| LodLevelJson { mesh: k.into(), screen_height: x.screen_height })).collect();
    to_value(LodJson { levels, cull: l.cull })
}

fn load_lod(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: LodJson = from_value(v)?;
    let mut levels = Vec::new();
    for l in j.levels {
        let mesh = ctx.assets.mesh(ctx.gfx, &l.mesh).map_err(|e| e.to_string())?;
        levels.push(goat_runtime::LodLevel { mesh, screen_height: l.screen_height });
    }
    insert(ctx, e, goat_runtime::LodGroup { levels, cull: j.cull })
}

/// `"StaticBatching": { "bake_lighting": true }`
fn save_static(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let s = *ctx.world.get::<&goat_runtime::StaticBatching>(e).ok()?;
    Some(if s.bake_lighting { serde_json::json!({ "bake_lighting": true }) } else { Value::Object(Default::default()) })
}
fn load_static(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let bake_lighting = v.get("bake_lighting").and_then(|b| b.as_bool()).unwrap_or(false);
    insert(ctx, e, goat_runtime::StaticBatching { bake_lighting })
}

// ------------------------------------------------------------------ lights, settings

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct LightJson {
    color: FileColor,
    intensity: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    range: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spot_angle: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inner_spot_angle: Option<f32>,
    /// Directional lights: casts shadow maps (default true).
    #[serde(skip_serializing_if = "Option::is_none")]
    shadows: Option<bool>,
    /// "realtime" | "mixed" (default) | "baked" (Unity's Light Mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
}

impl Default for LightJson {
    fn default() -> Self {
        LightJson { color: FileColor(Color::WHITE), intensity: 1.0, range: None, spot_angle: None, inner_spot_angle: None, shadows: None, mode: None }
    }
}

fn mode_json(m: LightMode) -> Option<String> {
    // always written: the Inspector edits the fields a component's JSON has
    Some(m.name().to_string())
}
fn mode_of(j: &LightJson) -> LightMode {
    j.mode.as_deref().map(LightMode::from_name).unwrap_or_default()
}

fn save_directional(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let l = *ctx.world.get::<&DirectionalLight>(e).ok()?;
    to_value(LightJson { color: FileColor(l.color), intensity: l.intensity, shadows: (!l.shadows).then_some(false), mode: mode_json(l.mode), ..Default::default() })
}
fn load_directional(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: LightJson = from_value(v)?;
    insert(ctx, e, DirectionalLight { color: j.color.0, intensity: j.intensity, shadows: j.shadows.unwrap_or(true), mode: mode_of(&j) })
}
fn save_point(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let l = *ctx.world.get::<&PointLight>(e).ok()?;
    to_value(LightJson { color: FileColor(l.color), intensity: l.intensity, range: Some(l.range), mode: mode_json(l.mode), ..Default::default() })
}
fn load_point(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: LightJson = from_value(v)?;
    insert(ctx, e, PointLight { color: j.color.0, intensity: j.intensity, range: j.range.unwrap_or(10.0), mode: mode_of(&j) })
}
fn save_spot(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let l = *ctx.world.get::<&SpotLight>(e).ok()?;
    to_value(LightJson { color: FileColor(l.color), intensity: l.intensity, range: Some(l.range), spot_angle: Some(l.spot_angle), inner_spot_angle: Some(l.inner_spot_angle), shadows: None, mode: mode_json(l.mode) })
}
fn load_spot(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: LightJson = from_value(v)?;
    let d = SpotLight::default();
    insert(
        ctx,
        e,
        SpotLight { color: j.color.0, intensity: j.intensity, range: j.range.unwrap_or(d.range), spot_angle: j.spot_angle.unwrap_or(d.spot_angle), inner_spot_angle: j.inner_spot_angle.unwrap_or(d.inner_spot_angle), mode: mode_of(&j) },
    )
}

#[derive(Serialize, Deserialize)]
struct FogJson {
    color: FileColor,
    start: f32,
    end: f32,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct RenderSettingsJson {
    ambient: FileColor,
    /// Hemisphere ambient: the color from below.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ambient_ground: Option<FileColor>,
    fog: Option<FogJson>,
    /// Screen effects: { "exposure", "contrast", "saturation", "tint", "vignette", "bloom", "bloom_threshold" }
    #[serde(default, skip_serializing_if = "Option::is_none")]
    post: Option<goat_gfx::PostFx>,
}

impl Default for RenderSettingsJson {
    fn default() -> Self {
        RenderSettingsJson { ambient: FileColor(RenderSettings::default().ambient), ambient_ground: None, fog: None, post: None }
    }
}

fn save_settings(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let s = *ctx.world.get::<&RenderSettings>(e).ok()?;
    to_value(RenderSettingsJson { ambient: FileColor(s.ambient), ambient_ground: s.ambient_ground.map(FileColor), post: s.post, fog: s.fog.map(|f| FogJson { color: FileColor(f.color), start: f.start, end: f.end }) })
}
fn load_settings(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: RenderSettingsJson = from_value(v)?;
    insert(ctx, e, RenderSettings { ambient: j.ambient.0, ambient_ground: j.ambient_ground.map(|c| c.0), post: j.post, fog: j.fog.map(|f| Fog { color: f.color.0, start: f.start, end: f.end }) })
}

/// `"LightProbeGroup": { "positions": [[x, y, z], ...] }`
fn save_probe_group(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let g = ctx.world.get::<&LightProbeGroup>(e).ok()?;
    Some(serde_json::json!({ "positions": g.positions.iter().map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>() }))
}
fn load_probe_group(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let positions = match v.get("positions").and_then(|p| p.as_array()) {
        Some(list) => list
            .iter()
            .filter_map(|p| {
                let a = p.as_array()?;
                let f = |i: usize| a.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
                Some(Vec3::new(f(0), f(1), f(2)))
            })
            .collect(),
        None => LightProbeGroup::default().positions,
    };
    insert(ctx, e, LightProbeGroup { positions })
}

/// `"ReflectionProbe": { "size": [10, 10, 10], "center": [0, 0, 0], "cubemap": "sky.cubemap.json", "importance": 1 }`
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct ReflectionProbeJson {
    size: [f32; 3],
    center: [f32; 3],
    /// A `.cubemap.json` or `builtin:sky`.
    cubemap: String,
    importance: i32,
}
impl Default for ReflectionProbeJson {
    fn default() -> Self {
        ReflectionProbeJson { size: [10.0; 3], center: [0.0; 3], cubemap: "builtin:sky".into(), importance: 1 }
    }
}
fn save_reflection_probe(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let r = *ctx.world.get::<&ReflectionProbe>(e).ok()?;
    let cubemap = r.cubemap.and_then(|c| ctx.assets.key_of_texture(c)).unwrap_or("").to_string();
    to_value(ReflectionProbeJson { size: r.size.into(), center: r.center.into(), cubemap, importance: r.importance })
}
fn load_reflection_probe(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: ReflectionProbeJson = from_value(v)?;
    let cubemap = if j.cubemap.is_empty() { None } else { Some(ctx.assets.cubemap(ctx.gfx, &j.cubemap).map_err(|e| e.to_string())?) };
    insert(ctx, e, ReflectionProbe { size: j.size.into(), center: j.center.into(), cubemap, importance: j.importance })
}

/// `"PropertyBlock": { "color": "#ff8080" }` (MaterialPropertyBlock `_Color`).
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct PropertyBlockJson {
    color: Option<FileColor>,
}
fn save_property_block(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let b = *ctx.world.get::<&goat_runtime::PropertyBlock>(e).ok()?;
    to_value(PropertyBlockJson { color: Some(FileColor(b.color)) })
}
fn load_property_block(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: PropertyBlockJson = from_value(v)?;
    insert(ctx, e, goat_runtime::PropertyBlock { color: j.color.map(|c| c.0).unwrap_or(goat_core::Color::WHITE) })
}

/// `"PostEffect": { "materials": ["materials/grayscale.mat.json"] }` (on a camera).
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct PostEffectJson {
    materials: Vec<String>,
}
fn save_post_effect(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let p = ctx.world.get::<&goat_runtime::PostEffect>(e).ok()?;
    to_value(PostEffectJson { materials: p.materials.iter().filter_map(|m| ctx.assets.key_of_material(*m).map(str::to_string)).collect() })
}
fn load_post_effect(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: PostEffectJson = from_value(v)?;
    let mut materials = Vec::new();
    for k in j.materials.iter().filter(|k| !k.is_empty()) {
        materials.push(ctx.assets.material(ctx.gfx, k).map_err(|e| e.to_string())?);
    }
    insert(ctx, e, goat_runtime::PostEffect { materials })
}

pub(crate) fn register(r: &mut SceneRegistry) {
    r.component(ComponentCodec { name: "Transform", save: save_transform, load: load_transform, remove: remove_typed::<Transform> })
        .component(ComponentCodec { name: "Camera", save: save_camera, load: load_camera, remove: remove_typed::<Camera> })
        .component(ComponentCodec { name: "MeshFilter", save: save_mesh_filter, load: load_mesh_filter, remove: remove_typed::<MeshFilter> })
        .component(ComponentCodec { name: "MeshRenderer", save: save_mesh_renderer, load: load_mesh_renderer, remove: remove_typed::<MeshRenderer> })
        .component(ComponentCodec { name: "SkinnedMeshRenderer", save: save_skinned_mesh_renderer, load: load_skinned_mesh_renderer, remove: remove_typed::<SkinnedMeshRenderer> })
        .component(ComponentCodec { name: "DirectionalLight", save: save_directional, load: load_directional, remove: remove_typed::<DirectionalLight> })
        .component(ComponentCodec { name: "PointLight", save: save_point, load: load_point, remove: remove_typed::<PointLight> })
        .component(ComponentCodec { name: "SpotLight", save: save_spot, load: load_spot, remove: remove_typed::<SpotLight> })
        .component(ComponentCodec { name: "RenderSettings", save: save_settings, load: load_settings, remove: remove_typed::<RenderSettings> })
        .component(ComponentCodec { name: "LodGroup", save: save_lod, load: load_lod, remove: remove_typed::<goat_runtime::LodGroup> })
        .component(ComponentCodec { name: "StaticBatching", save: save_static, load: load_static, remove: remove_typed::<goat_runtime::StaticBatching> })
        .component(ComponentCodec { name: "LightProbeGroup", save: save_probe_group, load: load_probe_group, remove: remove_typed::<LightProbeGroup> })
        .component(ComponentCodec { name: "ReflectionProbe", save: save_reflection_probe, load: load_reflection_probe, remove: remove_typed::<ReflectionProbe> })
        .component(ComponentCodec { name: "PropertyBlock", save: save_property_block, load: load_property_block, remove: remove_typed::<goat_runtime::PropertyBlock> })
        .component(ComponentCodec { name: "PostEffect", save: save_post_effect, load: load_post_effect, remove: remove_typed::<goat_runtime::PostEffect> });
}
