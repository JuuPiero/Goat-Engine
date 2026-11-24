//! Built-in components.

use goat_core::math::{Mat4, Quat, Vec3};
use goat_core::Color;
use goat_gfx::{Display, MaterialId, MeshId, Projection, Viewport};
use hecs::Entity;

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

/// Layer helpers: `layer_mask(&[0, 5])` = a culling mask with those layers.
pub fn layer_mask(layers: &[u8]) -> u32 {
    layers.iter().fold(0, |m, &l| m | (1u32 << l.min(31)))
}

/// Parallel light along the entity's forward (-Z), like the sun.
#[derive(Clone, Copy, Debug)]
pub struct DirectionalLight {
    pub color: Color,
    pub intensity: f32,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        DirectionalLight { color: Color::rgb(1.0, 0.97, 0.9), intensity: 1.0 }
    }
}

/// Light from the entity's position in every direction, fading out at `range`.
#[derive(Clone, Copy, Debug)]
pub struct PointLight {
    pub color: Color,
    pub intensity: f32,
    pub range: f32,
}

impl Default for PointLight {
    fn default() -> Self {
        PointLight { color: Color::WHITE, intensity: 1.0, range: 10.0 }
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
}

impl Default for SpotLight {
    fn default() -> Self {
        SpotLight { color: Color::WHITE, intensity: 1.0, range: 10.0, spot_angle: 30.0, inner_spot_angle: 20.0 }
    }
}

/// Scene-wide rendering settings (Unity's Lighting window: ambient, fog). Put it on any one
/// entity; without it the defaults apply.
#[derive(Clone, Copy, Debug)]
pub struct RenderSettings {
    pub ambient: Color,
    pub fog: Option<goat_gfx::Fog>,
}

impl Default for RenderSettings {
    fn default() -> Self {
        RenderSettings { ambient: Color::rgb(0.22, 0.24, 0.3), fog: None }
    }
}

/// Never saved in scenes / prefabs (helper entities rebuilt by their owner, e.g. sprite batches).
#[derive(Clone, Copy, Debug, Default)]
pub struct DontSave;
