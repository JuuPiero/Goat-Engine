//! GoatEngine's rendering API, independent of any GPU or platform.
//!
//! The engine never talks to Vulkan, citro3d... directly. It describes *what* to draw:
//! - resources created through a [`GraphicsBackend`]: [`MeshData`], [`TextureData`], [`MaterialDesc`];
//! - each frame, one [`RenderView`] per camera (a display, a viewport, matrices, lights and the
//!   draw items), submitted with [`GraphicsBackend::render`].
//!
//! Each backend crate (`goat-gfx-wgpu` = Vulkan on desktop, `goat-gfx-citro3d` = PICA200 on the
//! Nintendo 3DS, ...) turns that into its GPU's commands. The material model is the common subset
//! that every target can draw *well*: per-pixel Blinn-Phong lighting with one directional light +
//! ambient (fragment lighting on the 3DS), a base color texture, vertex colors, alpha blending.

pub mod mesh;
pub mod null;

use goat_core::handle::Handle;
use goat_core::math::{Mat4, Vec3};
use goat_core::Color;

pub use mesh::{MeshData, Vertex};
pub use null::NullBackend;

// ---------------------------------------------------------------------------------------------
// resources

/// Pixels of a texture, RGBA8, rows top to bottom.
#[derive(Clone, Debug)]
pub struct TextureData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub filter: Filter,
    pub wrap: Wrap,
    /// Generate a mip chain (smaller levels for distant / minified surfaces: less aliasing,
    /// faster texture fetches).
    pub mipmaps: bool,
    /// How the GPU stores it. Backends that can't store a format keep RGBA8 (desktop does).
    pub format: TextureFormat,
}

/// GPU storage of a texture. On the 3DS 16-bit formats halve VRAM / bandwidth.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum TextureFormat {
    #[default]
    Rgba8,
    /// 5-6-5, no alpha.
    Rgb565,
    /// 4-4-4-4.
    Rgba4,
    /// 5-5-5-1 (cut-out alpha).
    Rgba5551,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Filter {
    Nearest,
    #[default]
    Linear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Wrap {
    Clamp,
    #[default]
    Repeat,
}

impl TextureData {
    /// A checkerboard (handy default / debug texture). Sizes must be powers of two (8..1024) for the 3DS.
    pub fn checker(size: u32, cells: u32, a: Color, b: Color) -> Self {
        let (ca, cb) = (a.to_rgba8(), b.to_rgba8());
        let mut rgba = Vec::with_capacity((size * size * 4) as usize);
        let cell = (size / cells.max(1)).max(1);
        for y in 0..size {
            for x in 0..size {
                let c = if ((x / cell) + (y / cell)) % 2 == 0 { ca } else { cb };
                rgba.extend_from_slice(&c);
            }
        }
        TextureData { width: size, height: size, rgba, filter: Filter::Nearest, wrap: Wrap::Repeat, mipmaps: false, format: TextureFormat::Rgba8 }
    }

    /// Same settings, no pixels (what backends keep after uploading).
    pub fn clone_meta(&self) -> Self {
        TextureData { rgba: Vec::new(), ..*self }
    }

    /// Builder-style setters.
    pub fn with_mipmaps(mut self) -> Self {
        self.mipmaps = true;
        self.filter = Filter::Linear;
        self
    }
    pub fn with_format(mut self, format: TextureFormat) -> Self {
        self.format = format;
        self
    }

    /// The mip chain: level 0 = these pixels, each next level a 2×2 box filter of the previous,
    /// down to `min_size` (the 3DS can't go below 8×8). Returns (width, height, rgba) per level.
    pub fn mip_chain(&self, min_size: u32) -> Vec<(u32, u32, Vec<u8>)> {
        let mut levels = vec![(self.width, self.height, self.rgba.clone())];
        while self.mipmaps {
            let (w, h, src) = levels.last().unwrap();
            let (w, h) = (*w, *h);
            if w / 2 < min_size || h / 2 < min_size {
                break;
            }
            let (nw, nh) = (w / 2, h / 2);
            let mut dst = vec![0u8; (nw * nh * 4) as usize];
            for y in 0..nh {
                for x in 0..nw {
                    for c in 0..4 {
                        let at = |sx: u32, sy: u32| src[((sy * w + sx) * 4 + c) as usize] as u32;
                        let sum = at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1);
                        dst[((y * nw + x) * 4 + c) as usize] = ((sum + 2) / 4) as u8;
                    }
                }
            }
            levels.push((nw, nh, dst));
        }
        levels
    }

    /// A 1×1 texture of one color.
    pub fn solid(c: Color) -> Self {
        TextureData { width: 8, height: 8, rgba: c.to_rgba8().repeat(64), filter: Filter::Nearest, wrap: Wrap::Repeat, mipmaps: false, format: TextureFormat::Rgba8 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum BlendMode {
    #[default]
    Opaque,
    Alpha,
    Additive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum CullMode {
    None,
    #[default]
    Back,
    Front,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum LightingModel {
    /// No lighting: texture × vertex color × base color.
    Unlit,
    /// Per-pixel diffuse + Blinn-Phong specular.
    #[default]
    Lit,
}

/// How a surface looks (the portable material model).
#[derive(Clone, Debug)]
pub struct MaterialDesc {
    pub base_color: Color,
    pub texture: Option<TextureId>,
    pub lighting: LightingModel,
    /// Specular color (black = matte).
    pub specular: Color,
    /// Blinn-Phong exponent (1..256).
    pub shininess: f32,
    pub emissive: Color,
    /// Multiplies `emissive` (glowing windows, screens, lava cracks).
    pub emissive_texture: Option<TextureId>,
    /// A cube map reflected by the surface (see [`GraphicsBackend::create_cubemap`]).
    pub env_map: Option<TextureId>,
    /// 0 = no reflection, 1 = a mirror (the reflection replaces the lit color).
    pub reflectivity: f32,
    /// Alpha test: pixels with alpha below this are discarded (leaves, fences, hair cards);
    /// 0 = off. Works with opaque blending: no sorting, depth written.
    pub alpha_cutoff: f32,
    /// With `alpha_cutoff`: a 4×4 ordered dither around the cutoff for softer edges (fake
    /// alpha-to-coverage). Desktop only; the 3DS keeps the hard cut.
    pub alpha_dither: bool,
    /// Cel shading: diffuse light in this many flat bands (0 = smooth).
    pub toon_steps: u8,
    /// Rim light (fresnel): `rim_color` × (1 - N·V)^`rim_power` added at grazing angles
    /// (black = off).
    pub rim_color: Color,
    pub rim_power: f32,
    /// Toon outline: the back faces drawn again pushed out along the normals by this many
    /// world units, in `outline_color` (0 = off).
    pub outline_width: f32,
    pub outline_color: Color,
    /// A procedural texture instead of `texture` (animated water / fire / noise): the 3DS
    /// generates it in its proctex unit, desktops in the shader.
    pub procedural: Option<Procedural>,
    pub blend: BlendMode,
    pub cull: CullMode,
    pub depth_write: bool,
    /// Affected by the view's fog (off for HUD sprites / UI).
    pub fog: bool,
    /// Tested against the depth buffer (false: drawn over everything — editor gizmos).
    pub depth_test: bool,
}

impl Default for MaterialDesc {
    fn default() -> Self {
        MaterialDesc {
            base_color: Color::WHITE,
            texture: None,
            lighting: LightingModel::Lit,
            specular: Color::rgb(0.4, 0.4, 0.4),
            shininess: 32.0,
            emissive: Color::BLACK,
            emissive_texture: None,
            env_map: None,
            reflectivity: 0.0,
            alpha_cutoff: 0.0,
            alpha_dither: false,
            toon_steps: 0,
            rim_color: Color::BLACK,
            rim_power: 3.0,
            outline_width: 0.0,
            outline_color: Color::BLACK,
            procedural: None,
            blend: BlendMode::Opaque,
            cull: CullMode::Back,
            depth_write: true,
            fog: true,
            depth_test: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralKind {
    /// Rippling bands between the two colors (water, lava).
    #[default]
    Water,
    /// Flames rising along v: `color_a` at the base, `color_b` at the tips, alpha fading.
    Fire,
    /// Plain animated noise between the two colors (clouds, magic, static).
    Noise,
}

/// An animated texture computed on the GPU.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Procedural {
    pub kind: ProceduralKind,
    pub color_a: Color,
    pub color_b: Color,
    /// Detail (noise frequency).
    pub scale: f32,
    /// Animation speed.
    pub speed: f32,
}

impl Default for Procedural {
    fn default() -> Self {
        Procedural { kind: ProceduralKind::Water, color_a: Color::hex(0x1b4f8a), color_b: Color::hex(0x7fd0ff), scale: 4.0, speed: 1.0 }
    }
}

/// Backend-owned GPU resources, referred to by handles.
pub type MeshId = Handle<MeshData>;
pub type TextureId = Handle<TextureData>;
pub type MaterialId = Handle<MaterialDesc>;

// ---------------------------------------------------------------------------------------------
// displays and views

/// Where a view is drawn: a physical screen of the device (3DS: top / bottom), or the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Display {
    /// The main screen (desktop window, 3DS top screen).
    Main,
    /// A second screen (3DS bottom screen).
    Secondary,
}

/// A display's size in pixels as games see it (3DS top = 400×240, bottom = 320×240).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayInfo {
    pub display: Display,
    pub width: u32,
    pub height: u32,
    /// Supports stereoscopic 3D (3DS top screen).
    pub stereo: bool,
}

/// A rectangle of a display in normalized 0..1 coordinates (origin top-left).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Viewport {
    pub const FULL: Viewport = Viewport { x: 0.0, y: 0.0, width: 1.0, height: 1.0 };
}

/// Camera projection. Backends build the matrix themselves (each GPU has its own clip-space
/// conventions — the 3DS screens are rotated and its depth range is [-1, 0]).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Projection {
    Perspective { fov_y_radians: f32, near: f32, far: f32 },
    Orthographic { height: f32, near: f32, far: f32 },
}

/// Lights per view: the PICA200 has 8 hardware lights; every backend supports the same count.
pub const MAX_LIGHTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LightKind {
    /// Parallel rays along `direction` (sun).
    Directional,
    /// From `position` in every direction, fading to 0 at `range`.
    Point,
    /// A cone from `position` along `direction`: full inside `cos_inner`, 0 outside `cos_outer`
    /// (cosines of the half angles).
    Spot { cos_inner: f32, cos_outer: f32 },
}

/// One light, in world space. `color` already includes the intensity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub kind: LightKind,
    pub position: Vec3,
    /// Direction the light travels (normalized; directional / spot).
    pub direction: Vec3,
    pub color: Color,
    pub range: f32,
}

impl Light {
    /// Distance attenuation every backend uses: (1 − (d / range)²)², 0 beyond range.
    pub fn attenuation(distance: f32, range: f32) -> f32 {
        let x = (distance / range.max(1e-4)).min(1.0);
        let w = 1.0 - x * x;
        w * w
    }
    /// Spot cone factor (smoothstep between the outer and inner cosines).
    pub fn spot_factor(cos_angle: f32, cos_inner: f32, cos_outer: f32) -> f32 {
        let t = ((cos_angle - cos_outer) / (cos_inner - cos_outer).max(1e-4)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

/// The lights of a view: ambient + up to [`MAX_LIGHTS`] lights (the runtime picks the most
/// relevant ones: directional first, then the nearest point / spot lights).
#[derive(Clone, Debug, PartialEq)]
pub struct Lighting {
    /// Ambient light (with `ambient_ground`: the sky color, from above).
    pub ambient: Color,
    /// Hemisphere ambient (Unity's Gradient ambient): this color from below, blending to
    /// `ambient` from above along the surface normal.
    pub ambient_ground: Option<Color>,
    pub lights: Vec<Light>,
}

impl Default for Lighting {
    fn default() -> Self {
        Lighting { ambient: Color::rgb(0.22, 0.24, 0.3), ambient_ground: None, lights: Vec::new() }
    }
}

/// Linear distance fog (on the 3DS: the PICA200's fog unit). Objects only — clear with the fog
/// color for a matching background.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Fog {
    pub color: Color,
    /// View-space depth where fog starts / becomes opaque.
    pub start: f32,
    pub end: f32,
}

/// A bounding sphere in mesh space (frustum culling).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub center: Vec3,
    pub radius: f32,
}

impl Projection {
    /// An OpenGL-convention (right-handed, z in −1..1) matrix of this projection — for engine-side
    /// math (culling, picking). Backends build their own GPU matrices.
    pub fn gl_matrix(&self, aspect: f32) -> Mat4 {
        match *self {
            Projection::Perspective { fov_y_radians, near, far } => Mat4::perspective_rh_gl(fov_y_radians, aspect, near, far),
            Projection::Orthographic { height, near, far } => {
                let (h, w) = (height * 0.5, height * 0.5 * aspect);
                Mat4::orthographic_rh_gl(-w, w, -h, h, near, far)
            }
        }
    }
}

/// The 6 planes of a view-projection matrix (Gribb–Hartmann), for sphere tests.
#[derive(Clone, Copy, Debug)]
pub struct Frustum {
    planes: [goat_core::math::Vec4; 6],
}

impl Frustum {
    /// From a GL-convention clip matrix (`projection.gl_matrix(aspect) * view`).
    pub fn from_matrix(m: Mat4) -> Self {
        let r = [m.row(0), m.row(1), m.row(2), m.row(3)];
        let mut planes = [r[3] + r[0], r[3] - r[0], r[3] + r[1], r[3] - r[1], r[3] + r[2], r[3] - r[2]];
        for p in &mut planes {
            let len = p.truncate().length();
            if len > 0.0 {
                *p /= len;
            }
        }
        Frustum { planes }
    }
    /// False only when the sphere is entirely outside.
    pub fn intersects_sphere(&self, center: Vec3, radius: f32) -> bool {
        self.planes.iter().all(|p| p.truncate().dot(center) + p.w >= -radius)
    }
}

/// One object to draw.
#[derive(Clone, Copy, Debug)]
pub struct DrawItem {
    pub mesh: MeshId,
    pub material: MaterialId,
    /// Local → world.
    pub model: Mat4,
    /// Blended items draw in increasing order first, then back to front (sprites, UI).
    pub sorting_order: i32,
}

/// Everything one camera draws this frame.
#[derive(Clone, Debug)]
pub struct RenderView {
    pub display: Display,
    pub viewport: Viewport,
    /// World → camera (right-handed, camera looks down -Z).
    pub view: Mat4,
    pub projection: Projection,
    /// Clears the viewport first (None = keep what earlier views drew).
    pub clear: Option<Color>,
    /// Without a clear color: still reset the depth of the viewport, so this view draws over
    /// earlier ones (Unity's "Depth only"). A clear color always clears depth too.
    pub clear_depth: bool,
    pub lighting: Lighting,
    pub fog: Option<Fog>,
    pub items: Vec<DrawItem>,
    /// Higher draws later (several cameras on one display).
    pub depth: i32,
    /// Seconds since start (animated procedural materials).
    pub time: f32,
}

impl RenderView {
    /// The order every backend draws `items` in: opaque first, front to back (early depth
    /// rejection), then blended back to front. `blend` returns a material's blend mode (None =
    /// unknown material: the item is skipped). Returns indices into `items`.
    pub fn draw_order(&self, blend: impl Fn(MaterialId) -> Option<BlendMode>) -> Vec<usize> {
        let mut order: Vec<(usize, bool, i32, f32)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| {
                let blended = blend(it.material)? != BlendMode::Opaque;
                // view-space z of the object's origin (camera looks down -Z: larger = closer)
                Some((i, blended, if blended { it.sorting_order } else { 0 }, (self.view * it.model.w_axis).z))
            })
            .collect();
        // stable: equal keys keep the submission order (sprite batches rely on it)
        order.sort_by(|a, b| {
            let by_depth = if a.1 { a.3.partial_cmp(&b.3) } else { b.3.partial_cmp(&a.3) };
            a.1.cmp(&b.1).then(a.2.cmp(&b.2)).then(by_depth.unwrap_or(std::cmp::Ordering::Equal))
        });
        order.into_iter().map(|(i, _, _, _)| i).collect()
    }
}

/// Per-frame statistics a backend reports.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub draw_calls: u32,
    pub triangles: u32,
    /// GPU / CPU time of the frame in ms when the backend can measure it.
    pub gpu_ms: f32,
}

#[derive(Debug)]
pub enum GfxError {
    Unsupported(String),
    OutOfMemory(String),
    Backend(String),
}

impl std::fmt::Display for GfxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GfxError::Unsupported(s) => write!(f, "unsupported: {s}"),
            GfxError::OutOfMemory(s) => write!(f, "out of memory: {s}"),
            GfxError::Backend(s) => write!(f, "backend error: {s}"),
        }
    }
}
impl std::error::Error for GfxError {}

/// A GPU backend. Implemented once per graphics API; the engine only uses this trait.
pub trait GraphicsBackend {
    /// Name for logs ("wgpu/Vulkan", "citro3d/PICA200").
    fn name(&self) -> &str;
    fn displays(&self) -> Vec<DisplayInfo>;

    fn create_mesh(&mut self, mesh: &MeshData) -> Result<MeshId, GfxError>;
    fn create_texture(&mut self, tex: &TextureData) -> Result<TextureId, GfxError>;
    /// A cube map from 6 square faces in the order +X, -X, +Y, -Y, +Z, -Z (world axes; the
    /// face seen looking along that axis, up = +Y except ±Y faces: up = -Z / +Z).
    fn create_cubemap(&mut self, _faces: &[TextureData; 6]) -> Result<TextureId, GfxError> {
        Err(GfxError::Unsupported("cube maps".into()))
    }
    fn create_material(&mut self, mat: &MaterialDesc) -> Result<MaterialId, GfxError>;
    fn update_material(&mut self, id: MaterialId, mat: &MaterialDesc);
    /// A material's description (baking, tools). Backends keep it for drawing anyway.
    fn material_desc(&self, _id: MaterialId) -> Option<MaterialDesc> {
        None
    }
    fn destroy_mesh(&mut self, id: MeshId);
    fn destroy_texture(&mut self, id: TextureId);
    fn destroy_material(&mut self, id: MaterialId);

    /// A mesh's bounding sphere (the runtime culls with it). Backends keep it from
    /// [`MeshData::bounds`] at creation.
    fn mesh_bounds(&self, _id: MeshId) -> Option<Bounds> {
        None
    }

    /// Replaces a mesh's vertices / indices (geometry generated every frame: sprites, text,
    /// particles, UI). Backends reuse the GPU buffers when the data fits.
    fn update_mesh(&mut self, id: MeshId, mesh: &MeshData) -> Result<(), GfxError>;

    /// Replaces a texture's pixels (same size and format; mip level 0 only): dynamic atlases
    /// (font glyphs), procedural textures.
    fn update_texture(&mut self, id: TextureId, tex: &TextureData) -> Result<(), GfxError>;

    /// Draws a frame: every view, then presents every display.
    fn render(&mut self, views: &[RenderView]) -> FrameStats;
}

#[cfg(test)]
mod tests {
    use super::*;
    use goat_core::handle::Pool;

    #[test]
    fn draw_order_opaque_front_to_back_then_blended_back_to_front() {
        let mut meshes: Pool<MeshData> = Pool::new();
        let mut materials: Pool<MaterialDesc> = Pool::new();
        let mesh = meshes.insert(MeshData::default());
        let opaque = materials.insert(MaterialDesc::default());
        let alpha = materials.insert(MaterialDesc { blend: BlendMode::Alpha, ..Default::default() });
        let at = |z: f32| Mat4::from_translation(Vec3::new(0.0, 0.0, z));
        let item = |material, z| DrawItem { mesh, material, model: at(z), sorting_order: 0 };
        let view = RenderView {
            display: Display::Main,
            viewport: Viewport::FULL,
            view: Mat4::IDENTITY,
            projection: Projection::Perspective { fov_y_radians: 1.0, near: 0.1, far: 100.0 },
            clear: None,
            clear_depth: true,
            lighting: Lighting::default(),
            fog: None,
            // camera looks down -Z: z = -1 is near, -9 far
            items: vec![item(alpha, -2.0), item(opaque, -9.0), item(alpha, -8.0), item(opaque, -1.0)],
            depth: 0,
            time: 0.0,
        };
        let order = view.draw_order(|m| materials.get(m).map(|m| m.blend));
        assert_eq!(order, vec![3, 1, 2, 0]);
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;

    #[test]
    fn frustum_culls_spheres_outside() {
        let view = Mat4::look_at_rh(Vec3::ZERO, Vec3::NEG_Z, Vec3::Y);
        let proj = Projection::Perspective { fov_y_radians: 1.0, near: 0.1, far: 50.0 }.gl_matrix(1.0);
        let f = Frustum::from_matrix(proj * view);
        assert!(f.intersects_sphere(Vec3::new(0.0, 0.0, -10.0), 1.0));
        assert!(!f.intersects_sphere(Vec3::new(0.0, 0.0, 10.0), 1.0), "behind");
        assert!(!f.intersects_sphere(Vec3::new(0.0, 0.0, -60.0), 1.0), "past far");
        assert!(!f.intersects_sphere(Vec3::new(40.0, 0.0, -10.0), 1.0), "far right");
        assert!(f.intersects_sphere(Vec3::new(0.0, 0.0, -51.0), 2.0), "touches far");
    }

    #[test]
    fn light_falloff() {
        assert_eq!(Light::attenuation(0.0, 5.0), 1.0);
        assert_eq!(Light::attenuation(5.0, 5.0), 0.0);
        assert_eq!(Light::attenuation(9.0, 5.0), 0.0);
        assert!((Light::attenuation(2.5, 5.0) - 0.5625).abs() < 1e-6);
        assert_eq!(Light::spot_factor(1.0, 0.9, 0.8), 1.0);
        assert_eq!(Light::spot_factor(0.7, 0.9, 0.8), 0.0);
    }

    #[test]
    fn mip_chain_box_filters_down_to_min_size() {
        let t = TextureData::checker(32, 2, Color::WHITE, Color::BLACK).with_mipmaps();
        let levels = t.mip_chain(8);
        assert_eq!(levels.iter().map(|l| l.0).collect::<Vec<_>>(), vec![32, 16, 8]);
        assert_eq!(TextureData::checker(32, 2, Color::WHITE, Color::BLACK).mip_chain(1).len(), 1, "no mipmaps unless asked");
        // a 2×2-cell checker of 32 px: at 8×8 each cell is still pure
        assert_eq!(&levels[2].2[0..4], &[255, 255, 255, 255]);
    }
}
