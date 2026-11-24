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
    })
}

fn load_camera(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: CameraJson = from_value(v)?;
    let [x, y, w, h] = j.viewport;
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
        },
    )
}

// ------------------------------------------------------------------ MeshRenderer

#[derive(Serialize, Deserialize)]
struct MeshRendererJson {
    mesh: String,
    #[serde(default = "default_material")]
    material: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    visible: bool,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    layer: u8,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    sorting_order: i32,
}

fn default_material() -> String {
    "builtin:default".into()
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

fn save_mesh_renderer(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let r = *ctx.world.get::<&MeshRenderer>(e).ok()?;
    let (Some(mesh), Some(material)) = (ctx.assets.key_of_mesh(r.mesh), ctx.assets.key_of_material(r.material)) else {
        goat_core::warn!("scene save: a MeshRenderer uses a mesh / material made in code (no asset key): skipped");
        return None;
    };
    to_value(MeshRendererJson { mesh: mesh.into(), material: material.into(), visible: r.visible, layer: r.layer, sorting_order: r.sorting_order })
}

fn load_mesh_renderer(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: MeshRendererJson = from_value(v)?;
    let mesh = ctx.assets.mesh(ctx.gfx, &j.mesh).map_err(|e| e.to_string())?;
    let material = ctx.assets.material(ctx.gfx, &j.material).map_err(|e| e.to_string())?;
    let r = MeshRenderer { visible: j.visible, ..MeshRenderer::new(mesh, material).with_layer(j.layer).with_sorting_order(j.sorting_order) };
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

fn save_static(ctx: &SaveContext, e: Entity) -> Option<Value> {
    ctx.world.get::<&goat_runtime::StaticBatching>(e).ok().map(|_| Value::Object(Default::default()))
}
fn load_static(ctx: &mut LoadContext, e: Entity, _: &Value) -> Result<(), String> {
    insert(ctx, e, goat_runtime::StaticBatching)
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
}

impl Default for LightJson {
    fn default() -> Self {
        LightJson { color: FileColor(Color::WHITE), intensity: 1.0, range: None, spot_angle: None, inner_spot_angle: None }
    }
}

fn save_directional(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let l = *ctx.world.get::<&DirectionalLight>(e).ok()?;
    to_value(LightJson { color: FileColor(l.color), intensity: l.intensity, ..Default::default() })
}
fn load_directional(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: LightJson = from_value(v)?;
    insert(ctx, e, DirectionalLight { color: j.color.0, intensity: j.intensity })
}
fn save_point(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let l = *ctx.world.get::<&PointLight>(e).ok()?;
    to_value(LightJson { color: FileColor(l.color), intensity: l.intensity, range: Some(l.range), ..Default::default() })
}
fn load_point(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: LightJson = from_value(v)?;
    insert(ctx, e, PointLight { color: j.color.0, intensity: j.intensity, range: j.range.unwrap_or(10.0) })
}
fn save_spot(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let l = *ctx.world.get::<&SpotLight>(e).ok()?;
    to_value(LightJson { color: FileColor(l.color), intensity: l.intensity, range: Some(l.range), spot_angle: Some(l.spot_angle), inner_spot_angle: Some(l.inner_spot_angle) })
}
fn load_spot(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: LightJson = from_value(v)?;
    let d = SpotLight::default();
    insert(
        ctx,
        e,
        SpotLight { color: j.color.0, intensity: j.intensity, range: j.range.unwrap_or(d.range), spot_angle: j.spot_angle.unwrap_or(d.spot_angle), inner_spot_angle: j.inner_spot_angle.unwrap_or(d.inner_spot_angle) },
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
}

impl Default for RenderSettingsJson {
    fn default() -> Self {
        RenderSettingsJson { ambient: FileColor(RenderSettings::default().ambient), ambient_ground: None, fog: None }
    }
}

fn save_settings(ctx: &SaveContext, e: Entity) -> Option<Value> {
    let s = *ctx.world.get::<&RenderSettings>(e).ok()?;
    to_value(RenderSettingsJson { ambient: FileColor(s.ambient), ambient_ground: s.ambient_ground.map(FileColor), fog: s.fog.map(|f| FogJson { color: FileColor(f.color), start: f.start, end: f.end }) })
}
fn load_settings(ctx: &mut LoadContext, e: Entity, v: &Value) -> Result<(), String> {
    let j: RenderSettingsJson = from_value(v)?;
    insert(ctx, e, RenderSettings { ambient: j.ambient.0, ambient_ground: j.ambient_ground.map(|c| c.0), fog: j.fog.map(|f| Fog { color: f.color.0, start: f.start, end: f.end }) })
}

pub(crate) fn register(r: &mut SceneRegistry) {
    r.component(ComponentCodec { name: "Transform", save: save_transform, load: load_transform, remove: remove_typed::<Transform> })
        .component(ComponentCodec { name: "Camera", save: save_camera, load: load_camera, remove: remove_typed::<Camera> })
        .component(ComponentCodec { name: "MeshRenderer", save: save_mesh_renderer, load: load_mesh_renderer, remove: remove_typed::<MeshRenderer> })
        .component(ComponentCodec { name: "DirectionalLight", save: save_directional, load: load_directional, remove: remove_typed::<DirectionalLight> })
        .component(ComponentCodec { name: "PointLight", save: save_point, load: load_point, remove: remove_typed::<PointLight> })
        .component(ComponentCodec { name: "SpotLight", save: save_spot, load: load_spot, remove: remove_typed::<SpotLight> })
        .component(ComponentCodec { name: "RenderSettings", save: save_settings, load: load_settings, remove: remove_typed::<RenderSettings> })
        .component(ComponentCodec { name: "LodGroup", save: save_lod, load: load_lod, remove: remove_typed::<goat_runtime::LodGroup> })
        .component(ComponentCodec { name: "StaticBatching", save: save_static, load: load_static, remove: remove_typed::<goat_runtime::StaticBatching> });
}
