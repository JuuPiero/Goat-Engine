//! Built-in components.

use goat_core::math::{Mat4, Quat, Vec3};
use goat_core::Color;
use goat_gfx::{Display, MaterialId, MeshId, Projection, Viewport};
use hecs::{Entity, World};

/// A display name (Hierarchy label, lookups by name).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Name(pub String);

/// Position / rotation / scale relative to the parent (or the world).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Transform { position: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::ONE }
    }
}

impl Transform {
    pub fn from_position(p: Vec3) -> Self {
        Transform { position: p, ..Default::default() }
    }
    pub fn with_rotation(mut self, r: Quat) -> Self {
        self.rotation = r;
        self
    }
    pub fn with_scale(mut self, s: Vec3) -> Self {
        self.scale = s;
        self
    }
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }
    /// Turns so that -Z (forward) points at `target`.
    pub fn look_at(&mut self, target: Vec3, up: Vec3) {
        let m = Mat4::look_at_rh(self.position, target, up).inverse();
        self.rotation = Quat::from_mat4(&m).normalize();
    }
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }
    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }
    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }
}

/// The world matrix, computed every frame from [`Transform`] + [`Parent`] (read-only for games).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlobalTransform(pub Mat4);

impl Default for GlobalTransform {
    fn default() -> Self {
        GlobalTransform(Mat4::IDENTITY)
    }
}

/// Makes the entity a child of another (its Transform becomes relative to the parent).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parent(pub Entity);

/// Renders the scene from this entity's transform.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub display: Display,
    pub viewport: Viewport,
    pub projection: Projection,
    pub clear: Option<Color>,
    /// With `clear: None`: reset depth anyway, to draw over earlier cameras (HUDs, overlays).
    /// False = also keep their depth (draw into the same 3D scene).
    pub clear_depth: bool,
    /// Draw order among cameras of one display (higher = later).
    pub depth: i32,
    /// Bit i set = renders the objects of layer i (Unity's Culling Mask). Default: everything.
    pub culling_mask: u32,
    /// Draws into this render texture instead of `display` (Unity's Target Texture): minimaps,
    /// mirrors, picture-in-picture. Such cameras render before the displays' cameras.
    pub target_texture: Option<goat_gfx::TextureId>,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            display: Display::Main,
            viewport: Viewport::FULL,
            projection: Projection::Perspective { fov_y_radians: 60f32.to_radians(), near: 0.1, far: 200.0 },
            clear: Some(Color::rgb(0.11, 0.13, 0.18)),
            clear_depth: true,
            depth: 0,
            culling_mask: u32::MAX,
            target_texture: None,
        }
    }
}

/// Draws a mesh with a material at the entity's transform.
#[derive(Clone, Copy, Debug)]
pub struct MeshRenderer {
    pub mesh: MeshId,
    pub material: MaterialId,
    pub visible: bool,
    /// Layer 0..31: cameras draw the layers in their `culling_mask`.
    pub layer: u8,
    /// Blended objects draw in increasing order, then back to front (Unity's Order in Layer).
    pub sorting_order: i32,
}

impl MeshRenderer {
    pub fn new(mesh: MeshId, material: MaterialId) -> Self {
        MeshRenderer { mesh, material, visible: true, layer: 0, sorting_order: 0 }
    }
    pub fn with_layer(mut self, layer: u8) -> Self {
        self.layer = layer.min(31);
        self
    }
    pub fn with_sorting_order(mut self, order: i32) -> Self {
        self.sorting_order = order;
        self
    }
}

/// Level of detail (Unity's LOD Group) next to a [`MeshRenderer`]: the renderer draws the mesh
/// of the first level whose `screen_height` (the object's height as a fraction of the screen,
/// 0..1) is reached; below the last level it isn't drawn when `cull` is on. Picked per camera.
#[derive(Clone, Debug, PartialEq)]
pub struct LodGroup {
    /// Highest detail first, `screen_height` decreasing.
    pub levels: Vec<LodLevel>,
    /// Not drawn below the last level (Unity's Culled).
    pub cull: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodLevel {
    pub mesh: MeshId,
    pub screen_height: f32,
}

impl LodGroup {
    /// The mesh for an object covering `height` of the screen (None = culled).
    pub fn pick(&self, height: f32) -> Option<MeshId> {
        for l in &self.levels {
            if height >= l.screen_height {
                return Some(l.mesh);
            }
        }
        if self.cull {
            None
        } else {
            self.levels.last().map(|l| l.mesh)
        }
    }
}

/// On a root: its static descendants' meshes are merged per material when the game starts
/// (Unity's static batching): far fewer draw calls for level geometry. The merged objects must
/// not move afterwards. Done by `goat_assets::StaticBatchPlugin`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StaticBatching {
    /// Bake the scene's lights (ambient / hemisphere, directional, point, spot) into the merged
    /// vertices and draw them unlit: static geometry costs no fragment lighting, the hardware
    /// lights stay for moving objects. Lights changed later don't affect the baked meshes.
    pub bake_lighting: bool,
}

/// Layer helpers: `layer_mask(&[0, 5])` = a culling mask with those layers.
pub fn layer_mask(layers: &[u8]) -> u32 {
    layers.iter().fold(0, |m, &l| m | (1u32 << l.min(31)))
}

/// How a light is used (Unity's Light ▸ Mode).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LightMode {
    /// Lights everything at run time; not baked into static batches.
    Realtime,
    /// Baked into static batches (`StaticBatching::bake_lighting`) and real time for moving
    /// objects (the engine's behaviour before light modes existed).
    #[default]
    Mixed,
    /// Only baked: into static batches and into light probes ([`LightProbeGroup`]) for moving
    /// objects. Costs no hardware light at run time.
    Baked,
}

impl LightMode {
    pub fn realtime(self) -> bool {
        self != LightMode::Baked
    }
    /// Baked into static batches.
    pub fn baked_into_statics(self) -> bool {
        self != LightMode::Realtime
    }
    pub fn name(self) -> &'static str {
        match self {
            LightMode::Realtime => "realtime",
            LightMode::Mixed => "mixed",
            LightMode::Baked => "baked",
        }
    }
    pub fn from_name(s: &str) -> Self {
        match s {
            "realtime" => LightMode::Realtime,
            "baked" => LightMode::Baked,
            _ => LightMode::Mixed,
        }
    }
}

/// Parallel light along the entity's forward (-Z), like the sun.
#[derive(Clone, Copy, Debug)]
pub struct DirectionalLight {
    pub color: Color,
    pub intensity: f32,
    /// Casts shadow maps (the first such light; desktop shadow maps, on the 3DS the PICA's own
    /// shadow hardware: a 256² map around where the camera looks).
    pub shadows: bool,
    pub mode: LightMode,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        DirectionalLight { color: Color::rgb(1.0, 0.97, 0.9), intensity: 1.0, shadows: true, mode: LightMode::Mixed }
    }
}

/// Light from the entity's position in every direction, fading out at `range`.
#[derive(Clone, Copy, Debug)]
pub struct PointLight {
    pub color: Color,
    pub intensity: f32,
    pub range: f32,
    pub mode: LightMode,
}

impl Default for PointLight {
    fn default() -> Self {
        PointLight { color: Color::WHITE, intensity: 1.0, range: 10.0, mode: LightMode::Mixed }
    }
}

/// A cone of light along the entity's forward (-Z). Angles are full cone angles in degrees
/// (Unity's Spot Angle / Inner Spot Angle).
#[derive(Clone, Copy, Debug)]
pub struct SpotLight {
    pub color: Color,
    pub intensity: f32,
    pub range: f32,
    pub spot_angle: f32,
    pub inner_spot_angle: f32,
    pub mode: LightMode,
}

impl Default for SpotLight {
    fn default() -> Self {
        SpotLight { color: Color::WHITE, intensity: 1.0, range: 10.0, spot_angle: 30.0, inner_spot_angle: 20.0, mode: LightMode::Mixed }
    }
}

/// Light probes (Unity's Light Probe Group): points (in the entity's space) where the
/// [`LightMode::Baked`] lights and the ambient light are captured. Moving objects get the light of
/// the nearest probes (blended by distance) as an ambient color plus one directional light.
#[derive(Clone, Debug, PartialEq)]
pub struct LightProbeGroup {
    pub positions: Vec<Vec3>,
}

impl Default for LightProbeGroup {
    fn default() -> Self {
        // Unity's default: the 8 corners of a 1 m cube
        let mut positions = Vec::new();
        for i in 0..8 {
            positions.push(Vec3::new(if i & 1 == 0 { -0.5 } else { 0.5 }, if i & 2 == 0 { -0.5 } else { 0.5 }, if i & 4 == 0 { -0.5 } else { 0.5 }));
        }
        LightProbeGroup { positions }
    }
}

/// A reflection probe (Unity's Reflection Probe): objects whose center is inside the box
/// (`center` ± `size` / 2 from the entity, world axes) reflect `cubemap` instead of their
/// material's env map (materials with reflectivity > 0). Overlapping probes: higher
/// `importance` wins, then the smaller box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReflectionProbe {
    pub size: Vec3,
    pub center: Vec3,
    pub cubemap: Option<goat_gfx::TextureId>,
    pub importance: i32,
}

impl Default for ReflectionProbe {
    fn default() -> Self {
        ReflectionProbe { size: Vec3::splat(10.0), center: Vec3::ZERO, cubemap: None, importance: 1 }
    }
}

/// Scene-wide rendering settings (Unity's Lighting window: ambient, fog). Put it on any one
/// entity; without it the defaults apply.
#[derive(Clone, Copy, Debug)]
pub struct RenderSettings {
    /// Ambient light (the sky color with `ambient_ground`).
    pub ambient: Color,
    /// Hemisphere ambient: the color from below (Unity's Gradient ambient source).
    pub ambient_ground: Option<Color>,
    pub fog: Option<goat_gfx::Fog>,
    /// Screen effects (exposure, contrast, saturation, tint, vignette, bloom).
    pub post: Option<goat_gfx::PostFx>,
}

impl Default for RenderSettings {
    fn default() -> Self {
        RenderSettings { ambient: Color::rgb(0.22, 0.24, 0.3), ambient_ground: None, fog: None, post: None }
    }
}

/// The object is switched off (Unity's `SetActive(false)`): it and its children aren't drawn,
/// updated, simulated. Set with [`set_active`]; [`InactiveInHierarchy`] is derived from it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inactive;

/// Present while the object or an ancestor is [`Inactive`] (Unity's `!activeInHierarchy`).
/// Kept up to date by `propagate_transforms` (each frame and on demand): systems skip these
/// entities with `.without::<&InactiveInHierarchy>()`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InactiveInHierarchy;

/// A tag (Unity's GameObject.tag): "Player", "Enemy"… Untagged objects have none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tag(pub String);

/// Unity's `SetActive`: switches an object (and its children) on / off.
pub fn set_active(world: &mut World, e: Entity, active: bool) {
    if active {
        let _ = world.remove_one::<Inactive>(e);
    } else {
        let _ = world.insert_one(e, Inactive);
    }
    crate::engine::refresh_active(world);
}

/// Unity's `activeSelf`.
pub fn active_self(world: &World, e: Entity) -> bool {
    world.get::<&Inactive>(e).is_err()
}

/// Unity's `activeInHierarchy`.
pub fn active_in_hierarchy(world: &World, e: Entity) -> bool {
    let mut cur = Some(e);
    for _ in 0..256 {
        let Some(x) = cur else { return true };
        if world.get::<&Inactive>(x).is_ok() {
            return false;
        }
        cur = world.get::<&Parent>(x).ok().map(|p| p.0);
    }
    true
}

/// Objects with a tag (Unity's FindGameObjectsWithTag), active ones only.
pub fn find_with_tag(world: &World, tag: &str) -> Vec<Entity> {
    world.query::<&Tag>().without::<&InactiveInHierarchy>().iter().filter(|(_, t)| t.0 == tag).map(|(e, _)| e).collect()
}
/// Never saved in scenes / prefabs (helper entities rebuilt by their owner, e.g. sprite batches).
#[derive(Clone, Copy, Debug, Default)]
pub struct DontSave;
