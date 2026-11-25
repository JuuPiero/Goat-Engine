//! The citro3d implementation (3DS only). All FFI is here.

use std::collections::HashMap;
use std::ffi::c_void;

use citro3d_sys as c3d;
use ctru_sys as ctru;
use goat_core::handle::Pool;
use goat_core::math::{Mat4, Vec3};
use goat_core::Color;
use goat_gfx::*;

use crate::tile_texture;

static SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/standard.shbin"));
static SHADER_REFLECT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/reflect.shbin"));
static SHADER_OUTLINE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/outline.shbin"));
static SHADER_RECEIVE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/receive.shbin"));
static SHADER_SHADOW: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shadow.shbin"));

/// Vertex shader programs: 0 = standard (base texture on unit 0, emissive map on unit 1),
/// 1 = reflect (cube map on unit 0 from the reflection vector, base texture on unit 1),
/// 2 = outline (pushed along the normals, flat color).
/// Programs: 0 standard, 1 reflect, 2 outline, 3 shadow receiver, 4 shadow caster.
const PROGRAMS: usize = 5;
const PROGRAM_RECEIVE: usize = 3;
const PROGRAM_CASTER: usize = 4;
/// Uniform slots of `locs`.
const LOC_UV_ST: usize = 4;
const LOC_LIGHT_MTX: usize = 5;
const LOC_LIGHT_MVP: usize = 6;
/// Half size of the square the shadow map covers (world units) and the depth range from the
/// light's eye point to the far side.
const SHADOW_EXTENT: f32 = 10.0;
const SHADOW_DEPTH: f32 = 60.0;

/// Debug switches set at build time: `GOAT_3DS_DEBUG=nodraw,nodrawcall,unlit,notex,nossaa`
/// (comma-separated) — for bisecting GPU problems.
fn debug(flag: &str) -> bool {
    option_env!("GOAT_3DS_DEBUG").is_some_and(|v| v.split(',').any(|f| f == flag))
}

// GX_TRANSFER_IN_FORMAT(RGBA8) | GX_TRANSFER_OUT_FORMAT(RGB8), no flip, untiled output
const TRANSFER_FLAGS: u32 = (0 << 8) | (1 << 12);
// … | GX_TRANSFER_SCALING(GX_TRANSFER_SCALE_XY): 2×2 box downscale (supersampling)
const TRANSFER_FLAGS_SSAA: u32 = TRANSFER_FLAGS | (2 << 24);
// … | GX_TRANSFER_SCALING(GX_TRANSFER_SCALE_X): 2×1 downscale — the stereo eyes' antialiasing
// (along the screen's vertical, the GPU's x): both eyes fit in VRAM where 2×2 wouldn't.
const TRANSFER_FLAGS_AA_X: u32 = TRANSFER_FLAGS | (1 << 24);

/// A linear-memory allocation the GPU reads.
struct LinearBuf {
    ptr: *mut c_void,
    capacity: usize,
}

impl LinearBuf {
    fn alloc(size: usize) -> Result<LinearBuf, GfxError> {
        let capacity = size.max(16);
        // SAFETY: plain allocation, checked.
        let ptr = unsafe { ctru::linearAlloc(capacity) };
        if ptr.is_null() {
            return Err(GfxError::OutOfMemory("linear heap".into()));
        }
        Ok(LinearBuf { ptr, capacity })
    }
    /// Copies bytes in (growing when needed) and flushes them for the GPU.
    fn write(&mut self, bytes: &[u8]) -> Result<(), GfxError> {
        if bytes.len() > self.capacity {
            *self = LinearBuf::alloc(bytes.len().next_power_of_two())?;
        }
        // SAFETY: the buffer holds at least bytes.len() bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.ptr as *mut u8, bytes.len());
            let _ = ctru::GSPGPU_FlushDataCache(self.ptr, bytes.len().max(1) as u32);
        }
        Ok(())
    }
}

impl Drop for LinearBuf {
    fn drop(&mut self) {
        // SAFETY: allocated with linearAlloc, freed once.
        unsafe { ctru::linearFree(self.ptr) }
    }
}

struct Mesh {
    vertices: LinearBuf,
    indices: LinearBuf,
    index_count: i32,
    bounds: Bounds,
    /// Updated meshes are double-buffered: the GPU may still read the previous frame's buffers
    /// while the CPU writes the next ones (`update_mesh` writes here, then swaps).
    back: Option<(LinearBuf, LinearBuf)>,
}

impl Mesh {
    fn vertices(&self) -> *mut c_void {
        self.vertices.ptr
    }
    fn indices(&self) -> *mut c_void {
        self.indices.ptr
    }
}

struct Texture {
    tex: Box<c3d::C3D_Tex>,
    /// Cube maps: the face table citro3d keeps a pointer to (held, never read).
    #[allow(dead_code)]
    cube: Option<Box<c3d::C3D_TexCube>>,
    /// Render textures: the target drawing into this (VRAM) texture; null otherwise.
    target: *mut c3d::C3D_RenderTarget,
}

impl Drop for Texture {
    fn drop(&mut self) {
        // SAFETY: initialised by C3D_TexInit* in create_texture / create_render_target; the
        // target (if any) goes first.
        unsafe {
            if !self.target.is_null() {
                c3d::C3D_RenderTargetDelete(self.target);
            }
            c3d::C3D_TexDelete(&mut *self.tex)
        }
    }
}

/// GPU state citro3d keeps *pointers* to while bound: it must never move (boxed, and the
/// lookup tables boxed individually — a HashMap moves its values when it grows).
struct Fixed {
    programs: [c3d::shaderProgram_s; PROGRAMS],
    light_env: c3d::C3D_LightEnv,
    lights: [c3d::C3D_Light; MAX_LIGHTS],
    specular_luts: HashMap<u32, Box<c3d::C3D_LightLut>>,
    attenuation_luts: HashMap<u32, Box<c3d::C3D_LightLutDA>>,
    spot_luts: HashMap<(i32, i32), Box<c3d::C3D_LightLut>>,
    fog_luts: HashMap<(u32, u32, u32, u32, bool), Box<c3d::C3D_FogLut>>,
    /// Procedural texture unit (unit 3) state and its tables.
    proctex: c3d::C3D_ProcTex,
    pt_noise: c3d::C3D_ProcTexLut,
    pt_rgb: c3d::C3D_ProcTexLut,
    pt_alpha: c3d::C3D_ProcTexLut,
    pt_color: c3d::C3D_ProcTexColorLut,
}

pub struct Citro3dBackend {
    dvlbs: [*mut ctru::DVLB_s; PROGRAMS],
    fixed: Box<Fixed>,
    /// Uniform locations per program: projection, modelView, viewInv, outline, uvST, lightMtx,
    /// lightMvp (-1 = none).
    locs: [[i32; 7]; PROGRAMS],
    loc_projection: i32,
    loc_model_view: i32,
    /// The program bound now and the view's matrices (re-uploaded when switching program).
    bound_program: usize,
    view_proj: Option<c3d::C3D_Mtx>,
    view_inv: [[f32; 4]; 3],
    top_ssaa: *mut c3d::C3D_RenderTarget,
    /// The eye targets are twice as wide (2×1 AA through the display transfer).
    eyes_aa: bool,
    top_left: *mut c3d::C3D_RenderTarget,
    top_right: *mut c3d::C3D_RenderTarget,
    bottom: *mut c3d::C3D_RenderTarget,
    /// Full-viewport quad in clip space (clears of partial viewports).
    clear_quad: Option<Mesh>,
    meshes: Pool<MeshData>,
    gpu_meshes: HashMap<u32, Mesh>,
    textures: Pool<TextureData>,
    gpu_textures: HashMap<u32, Texture>,
    /// Texture ids that are cube maps.
    cubes: std::collections::HashSet<u32>,
    materials: Pool<MaterialDesc>,
    current_lut: u32,
    /// Rim LUT bound on D1 (0 = none).
    current_rim: u32,
    /// The current view has fog (materials with `fog: false` turn it off for their draws).
    view_fog: bool,
    /// Seconds of the view being drawn (procedural animation).
    view_time: f32,
    /// Color grading of the view (exposure × tint, texenv stage 4).
    view_grade: Option<Color>,
    /// The light probe light currently set over the view's lights (None = the view's ambient).
    probe_light: Option<LightProbeSample>,
    /// uv tiling / offset of texture units 0 / 1 (the standard vertex shader's `uvST`).
    uv_st: [[f32; 4]; 2],
    /// The PICA's shadow map (None: no VRAM left for it).
    shadow: Option<ShadowMap>,
    /// This frame's shadow: world → (s, t, depth) of the map, and the light it follows.
    shadow_tex_mtx: Option<Mat4>,
    shadow_dir: Option<Vec3>,
    /// The view being drawn receives the shadow through this light (index in its lights).
    view_shadow: Option<usize>,
    stereo: bool,
    /// The view being drawn goes into a render texture (not a rotated screen: plain projections,
    /// viewport in texture pixels).
    into_texture: bool,
    /// Bound on texture units 1 / 2 when they have nothing (see `unbind_texture`).
    white: Option<Texture>,
}

/// A shadow texture (RGBA8 = 24-bit depth + 8-bit intensity, GPU_TEX_SHADOW_2D) and the render
/// target drawing into it.
struct ShadowMap {
    tex: Box<c3d::C3D_Tex>,
    target: *mut c3d::C3D_RenderTarget,
    size: u16,
}

const UV_IDENTITY: [[f32; 4]; 2] = [[1.0, 1.0, 0.0, 0.0]; 2];

impl ShadowMap {
    /// A VRAM shadow texture: 256² if it fits, else 128².
    fn new() -> Option<ShadowMap> {
        for size in [256u16, 128] {
            // SAFETY: citro3d allocates the texture in VRAM; failures are reported.
            unsafe {
                let mut tex: Box<c3d::C3D_Tex> = Box::new(std::mem::zeroed());
                if !c3d::C3D_TexInitShadow(&mut *tex, size, size) {
                    continue;
                }
                c3d::C3D_TexSetFilter(&mut *tex, ctru::GPU_NEAREST, ctru::GPU_NEAREST);
                c3d::C3D_TexSetWrap(&mut *tex, ctru::GPU_CLAMP_TO_EDGE, ctru::GPU_CLAMP_TO_EDGE);
                let target = c3d::C3D_RenderTargetCreateFromTex(&mut *tex, ctru::GPU_TEXFACE_2D, 0, c3d::C3D_DEPTHTYPE { __i: -1 });
                if target.is_null() {
                    c3d::C3D_TexDelete(&mut *tex);
                    continue;
                }
                goat_core::info!("shadow map: {size}×{size}");
                return Some(ShadowMap { tex, target, size });
            }
        }
        goat_core::warn!("shadow map: no VRAM left (blob shadows only)");
        None
    }
}

fn depth24() -> c3d::C3D_DEPTHTYPE {
    c3d::C3D_DEPTHTYPE { __e: ctru::GPU_RB_DEPTH24_STENCIL8 }
}

/// glam (column-major) → citro3d (rows stored w z y x).
fn to_c3d(m: Mat4) -> c3d::C3D_Mtx {
    let mut out: c3d::C3D_Mtx = unsafe { std::mem::zeroed() };
    let cols = m.to_cols_array_2d();
    for r in 0..4 {
        for c in 0..4 {
            // SAFETY: the `m` view of the union covers all 16 floats.
            unsafe { out.m[r * 4 + (3 - c)] = cols[c][r] };
        }
    }
    out
}

/// 0xRRGGBBAA (render target clears).
fn rgba_u32(c: Color) -> u32 {
    let [r, g, b, a] = c.to_rgba8();
    u32::from_be_bytes([r, g, b, a])
}
/// 0xAABBGGRR (texenv constants, fog color).
fn abgr_u32(c: Color) -> u32 {
    let [r, g, b, a] = c.to_rgba8();
    u32::from_le_bytes([r, g, b, a])
}

/// A light LUT table (256 values + 256 deltas) of `f` over [0, 1] (`signed`: [-1, 1], entries
/// 0..127 = [0, 1), 128..255 = [-1, 0) — the PICA's two's-complement indexing).
fn lut_table(f: impl Fn(f32) -> f32, signed: bool) -> [f32; 512] {
    let mut data = [0f32; 512];
    for j in 0..256 {
        let (x, step) = if signed {
            ((if j < 128 { j as f32 } else { j as f32 - 256.0 }) / 128.0, 1.0 / 128.0)
        } else {
            (j as f32 / 256.0, 1.0 / 256.0)
        };
        let v = f(x);
        data[j] = v;
        data[j + 256] = f(x + step) - v;
    }
    data
}

/// Distance attenuation for LightLutDA_Create (`arg0` = range).
unsafe extern "C" fn window_attenuation(dist: f32, range: f32, _unused: f32) -> f32 {
    Light::attenuation(dist, range)
}

fn index_bytes(mesh: &MeshData) -> &[u8] {
    // SAFETY: u16 slice viewed as bytes.
    unsafe { std::slice::from_raw_parts(mesh.indices.as_ptr() as *const u8, mesh.indices.len() * 2) }
}

fn mesh_upload(mesh: &MeshData) -> Result<Mesh, GfxError> {
    let (vb, ib) = (mesh.vertex_bytes(), index_bytes(mesh));
    let mut vertices = LinearBuf::alloc(vb.len())?;
    let mut indices = LinearBuf::alloc(ib.len())?;
    vertices.write(vb)?;
    indices.write(ib)?;
    Ok(Mesh { vertices, indices, index_count: mesh.indices.len() as i32, bounds: mesh.bounds(), back: None })
}

/// Target size in GPU pixels (the screens are rotated: width = the screen's height).
fn target_size(target: *mut c3d::C3D_RenderTarget) -> (f32, f32) {
    // SAFETY: a live render target owned by the backend.
    unsafe { ((*target).frameBuf.width as f32, (*target).frameBuf.height as f32) }
}

impl Citro3dBackend {
    /// Starts citro3d. The platform must have initialised the screens (`gfxInit*`) first.
    pub fn new() -> Result<Self, GfxError> {
        // SAFETY: plain citro3d / libctru initialisation calls with valid arguments; the pointers
        // they return are checked and owned by the backend until Drop.
        unsafe {
            // 1 MB of GPU commands per frame (the 256 KB default overflows — a panic — when the 3D
            // top screen draws a busy scene twice)
            if !c3d::C3D_Init(0x10_0000) {
                return Err(GfxError::Backend("C3D_Init failed".into()));
            }
            // VRAM (6 MB): 2D = an 800×480 supersampled target (3 MB); 3D = two 480×400 eyes with
            // 16-bit depth (2×1 antialiasing, 1.15 MB each) — or plain 240×400 eyes if they don't fit
            let top_ssaa = c3d::C3D_RenderTargetCreate(480, 800, ctru::GPU_RB_RGBA8, depth24());
            let bottom = c3d::C3D_RenderTargetCreate(240, 320, ctru::GPU_RB_RGBA8, depth24());
            let depth16 = c3d::C3D_DEPTHTYPE { __e: ctru::GPU_RB_DEPTH16 };
            let mut eyes_aa = !debug("noaa3d");
            let (mut top_left, mut top_right) = if eyes_aa {
                (c3d::C3D_RenderTargetCreate(480, 400, ctru::GPU_RB_RGBA8, depth16), c3d::C3D_RenderTargetCreate(480, 400, ctru::GPU_RB_RGBA8, depth16))
            } else {
                (std::ptr::null_mut(), std::ptr::null_mut())
            };
            if top_left.is_null() || top_right.is_null() {
                for t in [top_left, top_right] {
                    if !t.is_null() {
                        c3d::C3D_RenderTargetDelete(t);
                    }
                }
                eyes_aa = false;
                top_left = c3d::C3D_RenderTargetCreate(240, 400, ctru::GPU_RB_RGBA8, depth24());
                top_right = c3d::C3D_RenderTargetCreate(240, 400, ctru::GPU_RB_RGBA8, depth24());
            }
            if top_ssaa.is_null() || top_left.is_null() || top_right.is_null() || bottom.is_null() {
                return Err(GfxError::OutOfMemory("render targets (VRAM)".into()));
            }
            goat_core::info!("3D eyes: {}", if eyes_aa { "480×400, 2×1 antialiased" } else { "240×400" });
            c3d::C3D_RenderTargetSetOutput(bottom, ctru::GFX_BOTTOM, ctru::GFX_LEFT, TRANSFER_FLAGS);
            c3d::C3D_RenderTargetSetOutput(top_ssaa, ctru::GFX_TOP, ctru::GFX_LEFT, TRANSFER_FLAGS_SSAA);

            // shaders
            let mut dvlbs = [std::ptr::null_mut(); PROGRAMS];
            for (k, bin) in [SHADER, SHADER_REFLECT, SHADER_OUTLINE, SHADER_RECEIVE, SHADER_SHADOW].iter().enumerate() {
                let size = bin.len();
                let words = ctru::linearAlloc(size) as *mut u32;
                std::ptr::copy_nonoverlapping(bin.as_ptr(), words as *mut u8, size);
                dvlbs[k] = ctru::DVLB_ParseFile(words, size as u32);
                if dvlbs[k].is_null() {
                    return Err(GfxError::Backend("bad shader binary".into()));
                }
            }
            let mut fixed = Box::new(Fixed {
                programs: std::mem::zeroed(),
                light_env: std::mem::zeroed(),
                lights: std::mem::zeroed(),
                specular_luts: HashMap::new(),
                attenuation_luts: HashMap::new(),
                spot_luts: HashMap::new(),
                fog_luts: HashMap::new(),
                proctex: std::mem::zeroed(),
                pt_noise: std::mem::zeroed(),
                pt_rgb: std::mem::zeroed(),
                pt_alpha: std::mem::zeroed(),
                pt_color: std::mem::zeroed(),
            });
            // ctru-sys and citro3d-sys generate the same struct twice: same layout, cast.
            let mut locs = [[-1i32; 7]; PROGRAMS];
            for k in 0..PROGRAMS {
                let prog = &mut fixed.programs[k] as *mut c3d::shaderProgram_s as *mut ctru::shaderProgram_s;
                let _ = ctru::shaderProgramInit(prog);
                let _ = ctru::shaderProgramSetVsh(prog, (*dvlbs[k]).DVLE);
                let vsh = fixed.programs[k].vertexShader as *mut ctru::shaderInstance_s;
                for (j, name) in [c"projection", c"modelView", c"viewInv", c"outline", c"uvST", c"lightMtx", c"lightMvp"].iter().enumerate() {
                    locs[k][j] = ctru::shaderInstanceGetUniformLocation(vsh, name.as_ptr()) as i32;
                }
                let ok = if k == PROGRAM_CASTER { locs[k][LOC_LIGHT_MVP] >= 0 } else { locs[k][0] >= 0 && locs[k][1] >= 0 };
                if !ok {
                    return Err(GfxError::Backend("shader uniforms not found".into()));
                }
            }
            c3d::C3D_BindProgram(&mut fixed.programs[0]);
            let (loc_projection, loc_model_view) = (locs[0][0], locs[0][1]);

            // vertex layout = goat_gfx::Vertex
            let attr = c3d::C3D_GetAttrInfo();
            c3d::AttrInfo_Init(attr);
            c3d::AttrInfo_AddLoader(attr, 0, ctru::GPU_FLOAT, 3);
            c3d::AttrInfo_AddLoader(attr, 1, ctru::GPU_FLOAT, 3);
            c3d::AttrInfo_AddLoader(attr, 2, ctru::GPU_FLOAT, 2);
            c3d::AttrInfo_AddLoader(attr, 3, ctru::GPU_FLOAT, 4);

            // fragment lighting: the 8 hardware lights, enabled per view
            c3d::C3D_LightEnvInit(&mut fixed.light_env);
            let env: *mut c3d::C3D_LightEnv = &mut fixed.light_env;
            for light in fixed.lights.iter_mut() {
                c3d::C3D_LightInit(light, env);
                c3d::C3D_LightEnable(light, false);
            }
            for i in 0..6 {
                c3d::C3D_TexEnvInit(c3d::C3D_GetTexEnv(i));
            }

            let quad = MeshData {
                // z just inside the far plane (PICA clip z is [-w, 0], 0 = far: z = 0 itself is clipped)
                vertices: [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].iter().map(|&[x, y]| Vertex::new([x, y, -1e-4], [0.0, 0.0, 1.0], [0.0, 0.0])).collect(),
                indices: vec![0, 1, 2, 0, 2, 3],
            };
            let clear_quad = Some(mesh_upload(&quad)?);

            Ok(Citro3dBackend {
                dvlbs,
                fixed,
                locs,
                loc_projection,
                loc_model_view,
                bound_program: 0,
                view_proj: None,
                view_inv: [[0.0; 4]; 3],
                top_ssaa,
                eyes_aa,
                top_left,
                top_right,
                bottom,
                clear_quad,
                meshes: Pool::new(),
                gpu_meshes: HashMap::new(),
                textures: Pool::new(),
                gpu_textures: HashMap::new(),
                cubes: Default::default(),
                materials: Pool::new(),
                current_lut: u32::MAX,
                current_rim: u32::MAX,
                view_fog: false,
                view_time: 0.0,
                view_grade: None,
                probe_light: None,
                uv_st: UV_IDENTITY,
                shadow: ShadowMap::new(),
                shadow_tex_mtx: None,
                shadow_dir: None,
                view_shadow: None,
                stereo: false,
                into_texture: false,
                white: None,
            })
        }
    }

    /// Sets up the procedural texture unit for a material (None = off).
    fn set_procedural(&mut self, p: Option<&Procedural>) {
        let fixed = &mut *self.fixed;
        // SAFETY: the proctex state / tables live in `fixed` (boxed, never move while bound).
        unsafe {
            let Some(p) = p else {
                c3d::C3D_ProcTexBind(0, std::ptr::null_mut());
                return;
            };
            let pt: *mut c3d::C3D_ProcTex = &mut fixed.proctex;
            // the color table width is an 8-bit field: 256 would wrap to 0 (one flat color)
            c3d::C3D_ProcTexInit(pt, 0, 128);
            c3d::C3D_ProcTexNoiseEnable(pt, true);
            let t = self.view_time * p.speed;
            c3d::C3D_ProcTexNoiseCoefs(pt, c3d::C3D_ProcTex_UV as i32, 0.25, p.scale * 0.08, t * 0.12);
            c3d::C3D_ProcTexFilter(pt, ctru::GPU_PT_LINEAR);
            let lut = |f: &dyn Fn(f32) -> f32| -> [f32; 129] { std::array::from_fn(|i| f(i as f32 / 128.0).clamp(0.0, 1.0)) };
            c3d::ProcTexLut_FromArray(&mut fixed.pt_noise, lut(&|x| x * x * (3.0 - 2.0 * x)).as_ptr());
            let (a, b) = (p.color_a, p.color_b);
            let mix = |k: f32| Color::rgba(a.r + (b.r - a.r) * k, a.g + (b.g - a.g) * k, a.b + (b.b - a.b) * k, 1.0);
            let colors: [u32; 128] = std::array::from_fn(|i| abgr_u32(mix(i as f32 / 127.0)));
            match p.kind {
                ProceduralKind::Water => {
                    c3d::C3D_ProcTexClamp(pt, ctru::GPU_PT_MIRRORED_REPEAT, ctru::GPU_PT_MIRRORED_REPEAT);
                    c3d::C3D_ProcTexCombiner(pt, false, ctru::GPU_PT_ADD, ctru::GPU_PT_ADD);
                    c3d::ProcTexLut_FromArray(&mut fixed.pt_rgb, lut(&|x| x).as_ptr());
                }
                ProceduralKind::Fire => {
                    c3d::C3D_ProcTexClamp(pt, ctru::GPU_PT_CLAMP_TO_EDGE, ctru::GPU_PT_CLAMP_TO_EDGE);
                    c3d::C3D_ProcTexCombiner(pt, true, ctru::GPU_PT_V, ctru::GPU_PT_V);
                    c3d::ProcTexLut_FromArray(&mut fixed.pt_rgb, lut(&|x| 1.0 - x).as_ptr());
                    c3d::ProcTexLut_FromArray(&mut fixed.pt_alpha, lut(&|x| 1.0 - (1.0 - x) * (1.0 - x)).as_ptr());
                }
                ProceduralKind::Noise => {
                    c3d::C3D_ProcTexClamp(pt, ctru::GPU_PT_MIRRORED_REPEAT, ctru::GPU_PT_MIRRORED_REPEAT);
                    c3d::C3D_ProcTexCombiner(pt, false, ctru::GPU_PT_SQRT2, ctru::GPU_PT_SQRT2);
                    c3d::ProcTexLut_FromArray(&mut fixed.pt_rgb, lut(&|x| (x * 3.0).fract()).as_ptr());
                }
            }
            c3d::ProcTexColorLut_Write(&mut fixed.pt_color, colors.as_ptr(), 0, 128);
            c3d::C3D_ProcTexLutBind(ctru::GPU_LUT_NOISE, &mut fixed.pt_noise);
            c3d::C3D_ProcTexLutBind(ctru::GPU_LUT_RGBMAP, &mut fixed.pt_rgb);
            c3d::C3D_ProcTexLutBind(ctru::GPU_LUT_ALPHAMAP, &mut fixed.pt_alpha);
            c3d::C3D_ProcTexColorLutBind(&mut fixed.pt_color);
            c3d::C3D_ProcTexBind(0, pt);
        }
    }

    /// Binds shader program `k` (re-uploading the view's matrices to it).
    fn use_program(&mut self, k: usize) {
        if self.bound_program == k {
            return;
        }
        self.bound_program = k;
        self.loc_projection = self.locs[k][0];
        self.loc_model_view = self.locs[k][1];
        // SAFETY: programs live in `fixed` (boxed); uniform uploads of stack values.
        unsafe {
            c3d::C3D_BindProgram(&mut self.fixed.programs[k]);
            if let (Some(p), true) = (self.view_proj.as_ref(), self.loc_projection >= 0) {
                c3d::C3D_FVUnifMtx4x4(ctru::GPU_VERTEX_SHADER, self.loc_projection, p);
            }
            let inv = self.locs[k][2];
            if inv >= 0 {
                for (i, r) in self.view_inv.iter().enumerate() {
                    c3d::C3D_FVUnifSet(ctru::GPU_VERTEX_SHADER, inv + i as i32, r[0], r[1], r[2], r[3]);
                }
            }
        }
        // uniform registers are shared by the programs: put this one's uv transform back
        let st = self.uv_st;
        self.upload_uv_st(st);
    }

    /// uv tiling / offset of texture units 0 / 1 (programs with `uvST`: the standard one).
    fn set_uv_st(&mut self, st: [[f32; 4]; 2]) {
        if self.uv_st != st {
            self.upload_uv_st(st);
        }
    }

    fn upload_uv_st(&mut self, st: [[f32; 4]; 2]) {
        self.uv_st = st;
        let loc = self.locs.get(self.bound_program).map(|l| l[LOC_UV_ST]).unwrap_or(-1);
        if loc >= 0 {
            // SAFETY: uniform upload to the bound program's registers.
            unsafe {
                for (i, v) in st.iter().enumerate() {
                    c3d::C3D_FVUnifSet(ctru::GPU_VERTEX_SHADER, loc + i as i32, v[0], v[1], v[2], v[3]);
                }
            }
        }
    }

    /// The D0 lookup table: Blinn-Phong for a shininess, or (toon) flat bands of L·N — built
    /// once, cached.
    fn bind_specular_lut(&mut self, shininess: f32, toon_steps: u8) {
        let key = if toon_steps > 0 { 1000 + toon_steps as u32 } else { shininess.round().clamp(1.0, 256.0) as u32 };
        if self.current_lut == key {
            return;
        }
        let fixed = &mut *self.fixed;
        let lut = fixed.specular_luts.entry(key).or_insert_with(|| {
            let mut data = if toon_steps > 0 {
                let n = toon_steps as f32;
                lut_table(|x| if x <= 0.0 { 0.0 } else { ((x * n).round() / n).clamp(0.0, 1.0) }, true)
            } else {
                lut_table(|x| x.clamp(0.0, 1.0).powf(key as f32), false)
            };
            // SAFETY: plain struct init from a 512-float table.
            unsafe {
                let mut lut: Box<c3d::C3D_LightLut> = Box::new(std::mem::zeroed());
                c3d::LightLut_FromArray(&mut *lut, data.as_mut_ptr());
                lut
            }
        });
        let input = if toon_steps > 0 { ctru::GPU_LUTINPUT_LN } else { ctru::GPU_LUTINPUT_NH };
        // SAFETY: the LUT is boxed and kept in `fixed` (never moves while bound).
        // 4th argument = "negative": signed input (toon L·N), else |input| (Blinn N·H)
        unsafe { c3d::C3D_LightEnvLut(&mut fixed.light_env, ctru::GPU_LUT_D0, input, toon_steps > 0, &mut **lut) };
        self.current_lut = key;
    }

    /// The D1 lookup table: rim light (1 - N·V)^power, or none.
    fn bind_rim_lut(&mut self, power: Option<f32>) {
        let key = power.map(|p| (p * 16.0).round().max(1.0) as u32).unwrap_or(0);
        if self.current_rim == key {
            return;
        }
        self.current_rim = key;
        let fixed = &mut *self.fixed;
        if key == 0 {
            // SAFETY: unbinding a LUT.
            unsafe { c3d::C3D_LightEnvLut(&mut fixed.light_env, ctru::GPU_LUT_D1, ctru::GPU_LUTINPUT_NV, false, std::ptr::null_mut()) };
            return;
        }
        let lut = fixed.specular_luts.entry(2000 + key).or_insert_with(|| {
            let p = key as f32 / 16.0;
            let mut data = lut_table(|x| (1.0 - x.clamp(0.0, 1.0)).powf(p), false);
            // SAFETY: plain struct init.
            unsafe {
                let mut lut: Box<c3d::C3D_LightLut> = Box::new(std::mem::zeroed());
                c3d::LightLut_FromArray(&mut *lut, data.as_mut_ptr());
                lut
            }
        });
        // SAFETY: boxed in `fixed`.
        unsafe { c3d::C3D_LightEnvLut(&mut fixed.light_env, ctru::GPU_LUT_D1, ctru::GPU_LUTINPUT_NV, false, &mut **lut) };
    }

    /// The view's lights (view space) into the hardware lights; unused ones off.
    fn set_lights(&mut self, v: &RenderView) {
        let fixed = &mut *self.fixed;
        // Hemisphere ambient on the PICA: ambient = ground color, plus a light from straight up
        // of (sky - ground): exact facing up / down, a bit darker than a true hemisphere at the
        // horizon. It takes the first free light slot (skipped when all 8 are in use: the mean).
        let used = v.lighting.lights.len().min(MAX_LIGHTS);
        let hemi = v.lighting.ambient_ground.filter(|_| used < MAX_LIGHTS);
        let amb = match (v.lighting.ambient_ground, hemi) {
            (Some(g), Some(_)) => g,
            (Some(g), None) => {
                let s = v.lighting.ambient;
                Color::rgb((s.r + g.r) * 0.5, (s.g + g.g) * 0.5, (s.b + g.b) * 0.5)
            }
            _ => v.lighting.ambient,
        };
        // SAFETY: lights / LUTs live in `fixed` (boxed).
        unsafe {
            c3d::C3D_LightEnvAmbient(&mut fixed.light_env, amb.r, amb.g, amb.b);
        }
        for (i, slot) in fixed.lights.iter_mut().enumerate() {
            let light: *mut c3d::C3D_Light = slot;
            if let (Some(g), true) = (hemi, i == used) {
                let s = v.lighting.ambient;
                let up = v.view.transform_vector3(Vec3::Y).normalize_or(Vec3::Y);
                // SAFETY: as above.
                unsafe {
                    c3d::C3D_LightEnable(light, true);
                    c3d::C3D_LightShadowEnable(light, false);
                    let mut p: c3d::C3D_FVec = std::mem::zeroed();
                    p.c = [0.0, up.z, up.y, up.x];
                    c3d::C3D_LightPosition(light, &mut p);
                    c3d::C3D_LightDiffuse(light, (s.r - g.r).max(0.0), (s.g - g.g).max(0.0), (s.b - g.b).max(0.0));
                    c3d::C3D_LightSpecular0(light, 0.0, 0.0, 0.0);
                    let s1 = if i == 0 { 1.0 } else { 0.0 };
                    c3d::C3D_LightSpecular1(light, s1, s1, s1);
                    c3d::C3D_LightAmbient(light, 0.0, 0.0, 0.0);
                    c3d::C3D_LightDistAttnEnable(light, false);
                    c3d::C3D_LightSpotEnable(light, false);
                }
                continue;
            }
            let Some(l) = v.lighting.lights.get(i).filter(|_| i < MAX_LIGHTS) else {
                // SAFETY: as above.
                unsafe { c3d::C3D_LightEnable(light, false) };
                continue;
            };
            // light color registers are 8-bit (1.0 = 255; more wraps around): keep the hue, cap at 1
            let c = l.color;
            let peak = c.r.max(c.g).max(c.b).max(1.0);
            let c = Color::rgb(c.r / peak, c.g / peak, c.b / peak);
            // fragment lighting positions are in eye space, stored w z y x
            let (pos, w) = match l.kind {
                LightKind::Directional => ((v.view.transform_vector3(-l.direction)).normalize_or(Vec3::Y), 0.0),
                _ => (v.view.transform_point3(l.position), 1.0),
            };
            // SAFETY: as above.
            let shadowed = self.view_shadow == Some(i);
            unsafe {
                c3d::C3D_LightEnable(light, true);
                c3d::C3D_LightShadowEnable(light, shadowed);
                let mut p: c3d::C3D_FVec = std::mem::zeroed();
                p.c = [w, pos.z, pos.y, pos.x];
                c3d::C3D_LightPosition(light, &mut p);
                c3d::C3D_LightColor(light, c.r, c.g, c.b);
                // rim light (material specular1 × D1(N·V)) through one light only
                let s1 = if i == 0 { 1.0 } else { 0.0 };
                c3d::C3D_LightSpecular1(light, s1, s1, s1);
                c3d::C3D_LightAmbient(light, 0.0, 0.0, 0.0);
                if l.kind == LightKind::Directional {
                    c3d::C3D_LightDistAttnEnable(light, false);
                } else {
                    let key = (l.range * 64.0).round() as u32;
                    let lut = fixed.attenuation_luts.entry(key).or_insert_with(|| {
                        let mut lut: Box<c3d::C3D_LightLutDA> = Box::new(std::mem::zeroed());
                        let range = key as f32 / 64.0;
                        c3d::LightLutDA_Create(&mut *lut, Some(window_attenuation), 0.0, range, range, 0.0);
                        lut
                    });
                    c3d::C3D_LightDistAttn(light, &mut **lut);
                    c3d::C3D_LightDistAttnEnable(light, true);
                }
                if let LightKind::Spot { cos_inner, cos_outer } = l.kind {
                    let key = ((cos_inner * 1000.0) as i32, (cos_outer * 1000.0) as i32);
                    let lut = fixed.spot_luts.entry(key).or_insert_with(|| {
                        let mut data = lut_table(|x| Light::spot_factor(x, cos_inner, cos_outer), true);
                        let mut lut: Box<c3d::C3D_LightLut> = Box::new(std::mem::zeroed());
                        c3d::LightLut_FromArray(&mut *lut, data.as_mut_ptr());
                        lut
                    });
                    let d = v.view.transform_vector3(l.direction).normalize_or(Vec3::NEG_Z);
                    c3d::C3D_LightSpotDir(light, d.x, d.y, d.z);
                    c3d::C3D_LightSpotLut(light, &mut **lut);
                    c3d::C3D_LightSpotEnable(light, true);
                } else {
                    c3d::C3D_LightSpotEnable(light, false);
                }
            }
        }
    }

    /// An object's light probe light: its ambient replaces the view's (hemisphere included) and its
    /// directional light takes the first slot after the view's lights (folded into the ambient
    /// when all 8 are used). `None` puts the view's lighting back.
    fn set_probe_light(&mut self, v: &RenderView, probe: Option<LightProbeSample>) {
        if self.probe_light == probe {
            return;
        }
        self.probe_light = probe;
        let Some(p) = probe else {
            self.set_lights(v);
            return;
        };
        let used = v.lighting.lights.len().min(MAX_LIGHTS);
        let fixed = &mut *self.fixed;
        let cap = |c: Color| {
            let peak = c.r.max(c.g).max(c.b).max(1.0);
            Color::rgb(c.r / peak, c.g / peak, c.b / peak)
        };
        let amb = if used < MAX_LIGHTS { p.ambient } else { Color::rgb(p.ambient.r + p.color.r * 0.5, p.ambient.g + p.color.g * 0.5, p.ambient.b + p.color.b * 0.5) };
        let amb = cap(amb);
        // SAFETY: lights live in `fixed` (boxed).
        unsafe {
            c3d::C3D_LightEnvAmbient(&mut fixed.light_env, amb.r, amb.g, amb.b);
            if used < MAX_LIGHTS {
                let light: *mut c3d::C3D_Light = &mut fixed.lights[used];
                let d = v.view.transform_vector3(-p.direction).normalize_or(Vec3::Y);
                let c = cap(p.color);
                c3d::C3D_LightEnable(light, true);
                c3d::C3D_LightShadowEnable(light, false);
                let mut pos: c3d::C3D_FVec = std::mem::zeroed();
                pos.c = [0.0, d.z, d.y, d.x];
                c3d::C3D_LightPosition(light, &mut pos);
                c3d::C3D_LightDiffuse(light, c.r, c.g, c.b);
                c3d::C3D_LightSpecular0(light, 0.0, 0.0, 0.0);
                let s1 = if used == 0 { 1.0 } else { 0.0 };
                c3d::C3D_LightSpecular1(light, s1, s1, s1);
                c3d::C3D_LightAmbient(light, 0.0, 0.0, 0.0);
                c3d::C3D_LightDistAttnEnable(light, false);
                c3d::C3D_LightSpotEnable(light, false);
            }
        }
    }

    /// The shadow receiver program's model → shadow map matrix.
    fn upload_light_mtx(&self, model: &Mat4) {
        if self.bound_program != PROGRAM_RECEIVE {
            return;
        }
        let (Some(m), loc) = (self.shadow_tex_mtx, self.locs[PROGRAM_RECEIVE][LOC_LIGHT_MTX]) else { return };
        if loc >= 0 {
            // SAFETY: uniform upload.
            unsafe { c3d::C3D_FVUnifMtx4x4(ctru::GPU_VERTEX_SHADER, loc, &to_c3d(m * *model)) };
        }
    }

    /// Draws the opaque objects of the first main-screen view with a directional shadow light
    /// into the shadow map, from the light (orthographic, a square around where the camera
    /// looks). Depth only: the PICA keeps the nearest depth per texel.
    fn shadow_pass(&mut self, views: &[RenderView], stats: &mut FrameStats) {
        self.shadow_tex_mtx = None;
        self.shadow_dir = None;
        if debug("noshadow") {
            return;
        }
        let Some(v) = views.iter().find(|v| v.display == Display::Main && matches!(v.projection, Projection::Perspective { .. }) && v.lighting.shadow.is_some()) else { return };
        let Some(l) = v.lighting.shadow.and_then(|i| v.lighting.lights.get(i)).filter(|l| l.kind == LightKind::Directional) else { return };
        let Some((target, size)) = self.shadow.as_ref().map(|s| (s.target, s.size)) else { return };
        let cam = v.view.inverse();
        let focus = cam.w_axis.truncate() + cam.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Z) * (SHADOW_EXTENT * 0.8);
        let d = l.direction.normalize_or(Vec3::NEG_Y);
        let up = if d.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        // snap the focus to whole texels in light space (no shimmering when the camera moves)
        let raw = Mat4::look_at_rh(Vec3::ZERO, d, up);
        let texel = 2.0 * SHADOW_EXTENT / size as f32;
        let f = raw.transform_point3(focus);
        let snapped = raw.inverse().transform_point3(Vec3::new((f.x / texel).floor() * texel, (f.y / texel).floor() * texel, f.z));
        let view = Mat4::look_at_rh(snapped - d * (SHADOW_DEPTH * 0.5), snapped, up);
        // casters: x, y in −1..1, z = −depth (the w-buffer writes depth = −z: 0 near .. 1 far)
        let caster = Mat4::from_scale(Vec3::new(1.0 / SHADOW_EXTENT, 1.0 / SHADOW_EXTENT, 1.0 / SHADOW_DEPTH)) * view;
        // receivers: (s, t) in 0..1, depth like the casters'
        let tex = Mat4::from_translation(Vec3::new(0.5, 0.5, 0.0)) * Mat4::from_scale(Vec3::new(0.5 / SHADOW_EXTENT, 0.5 / SHADOW_EXTENT, -1.0 / SHADOW_DEPTH)) * view;
        self.shadow_tex_mtx = Some(tex);
        self.shadow_dir = Some(d);
        self.use_program(PROGRAM_CASTER);
        let loc = self.locs[PROGRAM_CASTER][LOC_LIGHT_MVP];
        // SAFETY: draws into the shadow target with fixed-function state set here.
        unsafe {
            c3d::C3D_RenderTargetClear(target, c3d::C3D_CLEAR_COLOR, 0xFFFF_FFFF, 0);
            c3d::C3D_FrameDrawOn(target);
            c3d::C3D_SetViewport(0, 0, size as u32, size as u32);
            c3d::C3D_FragOpMode(ctru::GPU_FRAGOPMODE_SHADOW);
            c3d::C3D_DepthMap(false, -1.0, 0.0);
            c3d::C3D_DepthTest(false, ctru::GPU_ALWAYS, ctru::GPU_WRITE_COLOR);
            c3d::C3D_CullFace(ctru::GPU_CULL_NONE);
            c3d::C3D_AlphaTest(false, ctru::GPU_ALWAYS, 0);
            c3d::C3D_FogGasMode(ctru::GPU_NO_FOG, ctru::GPU_PLAIN_DENSITY, false);
            c3d::C3D_LightEnvBind(std::ptr::null_mut());
            self.unbind_texture(0);
            self.unbind_texture(1);
            // black: intensity (green) 0 = write the depth only
            let env0 = c3d::C3D_GetTexEnv(0);
            c3d::C3D_TexEnvInit(env0);
            c3d::C3D_TexEnvSrc(env0, c3d::C3D_Both, ctru::GPU_CONSTANT, 0, 0);
            c3d::C3D_TexEnvFunc(env0, c3d::C3D_Both, ctru::GPU_REPLACE);
            c3d::C3D_TexEnvColor(env0, 0xFF00_0000);
            for k in 1..6 {
                c3d::C3D_TexEnvInit(c3d::C3D_GetTexEnv(k));
            }
            c3d::C3D_TexEnvBufUpdate(c3d::C3D_Both as i32, 0);
        }
        for it in &v.items {
            let Some(m) = self.materials.get(it.material) else { continue };
            if m.blend != BlendMode::Opaque || !m.depth_write || !m.depth_test {
                continue;
            }
            let Some(mesh) = self.gpu_meshes.get(&it.mesh.index()) else { continue };
            if mesh.index_count == 0 {
                continue;
            }
            // outside the shadow square: skip
            if let Some(b) = self.mesh_bounds(it.mesh) {
                let c = view.transform_point3(it.model.transform_point3(b.center));
                let s = it.model.x_axis.truncate().length().max(it.model.y_axis.truncate().length()).max(it.model.z_axis.truncate().length());
                let r = b.radius * s;
                if c.x.abs() > SHADOW_EXTENT + r || c.y.abs() > SHADOW_EXTENT + r || -c.z + r < 0.0 || -c.z - r > SHADOW_DEPTH {
                    continue;
                }
            }
            let mvp = to_c3d(caster * it.model);
            let (vbo, ibo, count) = (mesh.vertices(), mesh.indices(), mesh.index_count);
            // SAFETY: buffers live in linear memory owned by `gpu_meshes`.
            unsafe {
                c3d::C3D_FVUnifMtx4x4(ctru::GPU_VERTEX_SHADER, loc, &mvp);
                let buf = c3d::C3D_GetBufInfo();
                c3d::BufInfo_Init(buf);
                c3d::BufInfo_Add(buf, vbo, Vertex::SIZE as isize, 4, 0x3210);
                c3d::C3D_DrawElements(ctru::GPU_TRIANGLES, count, c3d::C3D_UNSIGNED_SHORT as i32, ibo);
            }
            stats.draw_calls += 1;
        }
        // SAFETY: back to normal fragment operations.
        unsafe { c3d::C3D_FragOpMode(ctru::GPU_FRAGOPMODE_GL) };
    }

    /// Linear fog through the PICA fog unit (LUT indexed by depth: 0 = far, 1 = near).
    fn set_fog(&mut self, v: &RenderView) {
        self.view_fog = v.fog.is_some();
        let Some(fog) = v.fog else {
            // SAFETY: fixed-function state.
            unsafe { c3d::C3D_FogGasMode(ctru::GPU_NO_FOG, ctru::GPU_PLAIN_DENSITY, false) };
            return;
        };
        // depth is linear in view z (w-buffer, see draw_view): depth 1 = near, 0 = far
        let (near, far) = match v.projection {
            Projection::Perspective { near, far, .. } | Projection::Orthographic { near, far, .. } => (near, far),
        };
        let q = |f: f32| (f * 16.0).round().max(0.0) as u32;
        let key = (q(near), q(far), q(fog.start), q(fog.end), true);
        let lut = self.fixed.fog_luts.entry(key).or_insert_with(|| {
            let z_of = |depth: f32| far - depth * (far - near);
            let vis = |depth: f32| ((fog.end - z_of(depth)) / (fog.end - fog.start).max(1e-3)).clamp(0.0, 1.0);
            let mut data = [0f32; 256];
            for i in 0..128 {
                let a = vis(i as f32 / 128.0);
                data[i] = a;
                data[i + 128] = vis((i + 1) as f32 / 128.0) - a;
            }
            // SAFETY: struct init from a 256-float table.
            unsafe {
                let mut lut: Box<c3d::C3D_FogLut> = Box::new(std::mem::zeroed());
                c3d::FogLut_FromArray(&mut *lut, data.as_ptr());
                lut
            }
        });
        // SAFETY: the LUT is boxed in `fixed`.
        unsafe {
            c3d::C3D_FogGasMode(ctru::GPU_FOG, ctru::GPU_PLAIN_DENSITY, false);
            c3d::C3D_FogColor(abgr_u32(fog.color) & 0x00ff_ffff);
            c3d::C3D_FogLutBind(&mut **lut);
        }
    }

    /// Nothing on a texture unit. citro3d's C3D_TexBind reads the texture's type for units 1 / 2
    /// (a NULL there is a read of address 0xC: a data abort on hardware), so those units get an
    /// 8×8 white texture instead; unit 0 takes NULL.
    fn unbind_texture(&mut self, unit: i32) {
        // SAFETY: the white texture is boxed and lives as long as the backend.
        unsafe {
            if unit == 0 {
                c3d::C3D_TexBind(0, std::ptr::null_mut());
                return;
            }
            if self.white.is_none() {
                let mut tex: Box<c3d::C3D_Tex> = Box::new(std::mem::zeroed());
                if !c3d::C3D_TexInit(&mut *tex, 8, 8, ctru::GPU_RGBA8) {
                    return;
                }
                let px = vec![0xFFu8; 8 * 8 * 4];
                c3d::C3D_TexLoadImage(&mut *tex, px.as_ptr() as *const c_void, ctru::GPU_TEXFACE_2D, 0);
                c3d::C3D_TexFlush(&mut *tex);
                self.white = Some(Texture { tex, cube: None, target: std::ptr::null_mut() });
            }
            if let Some(w) = self.white.as_mut() {
                c3d::C3D_TexBind(unit, &mut *w.tex);
            }
        }
    }

    /// A render texture's id once its texture and target exist (create_render_target).
    unsafe fn finish_render_target(&mut self, mut tex: Box<c3d::C3D_Tex>, target: *mut c3d::C3D_RenderTarget, w: u32, h: u32) -> TextureId {
        c3d::C3D_TexSetFilter(&mut *tex, ctru::GPU_LINEAR, ctru::GPU_LINEAR);
        c3d::C3D_TexSetWrap(&mut *tex, ctru::GPU_CLAMP_TO_EDGE, ctru::GPU_CLAMP_TO_EDGE);
        c3d::C3D_RenderTargetClear(target, c3d::C3D_CLEAR_ALL, 0x000000ff, 0);
        let id = self.textures.insert(TextureData::rgba8(w, h, Vec::new()));
        self.gpu_textures.insert(id.index(), Texture { tex, cube: None, target });
        id
    }

    fn projection(&self, p: Projection, aspect: f32, eye: f32, slider: f32) -> c3d::C3D_Mtx {
        let mut m: c3d::C3D_Mtx = unsafe { std::mem::zeroed() };
        // SAFETY: writes a matrix in place.
        unsafe {
            match p {
                Projection::Perspective { fov_y_radians, near, far } if self.into_texture => c3d::Mtx_Persp(&mut m, fov_y_radians, aspect, near, far, false),
                Projection::Orthographic { height, near, far } if self.into_texture => {
                    let h = height * 0.5;
                    let w = h * aspect;
                    c3d::Mtx_Ortho(&mut m, -w, w, -h, h, near, far, false);
                }
                Projection::Perspective { fov_y_radians, near, far } => {
                    if eye != 0.0 {
                        c3d::Mtx_PerspStereoTilt(&mut m, fov_y_radians, aspect, near, far, eye * slider / 3.0, 2.0, false);
                    } else {
                        c3d::Mtx_PerspTilt(&mut m, fov_y_radians, aspect, near, far, false);
                    }
                }
                Projection::Orthographic { height, near, far } => {
                    let h = height * 0.5;
                    let w = h * aspect;
                    c3d::Mtx_OrthoTilt(&mut m, -w, w, -h, h, near, far, false);
                }
            }
        }
        m
    }

    fn set_material(&mut self, m: &MaterialDesc, tex: Option<*mut c3d::C3D_Tex>) {
        if let Some(p) = &m.tev {
            self.set_tev_material(m, p);
            self.set_render_state(m);
            // clip / discard: the program's alpha test
            if let Some(a) = p.alpha_test {
                // SAFETY: fixed-function state.
                unsafe {
                    if a.reference > 1.0 {
                        c3d::C3D_AlphaTest(true, ctru::GPU_NEVER, 0);
                    } else {
                        let r = (a.reference.clamp(0.0, 1.0) * 255.0).round() as i32;
                        c3d::C3D_AlphaTest(true, if a.greater_equal { ctru::GPU_GEQUAL } else { ctru::GPU_LEQUAL }, r);
                    }
                }
            }
            return;
        }
        let gpu = |id: Option<TextureId>, cube: bool, s: &mut Self| id.filter(|t| s.cubes.contains(&t.index()) == cube).and_then(|t| s.gpu_textures.get_mut(&t.index())).map(|t| &mut *t.tex as *mut c3d::C3D_Tex);
        let env_map = if m.reflectivity > 0.0 { gpu(m.env_map, true, self) } else { None };
        let emissive_map = if env_map.is_none() { gpu(m.emissive_texture, false, self) } else { None };
        // shadow receivers: lit, opaque-ish standard materials without a cube / emission map
        // (the shadow map takes texture unit 0)
        let receive = self.view_shadow.is_some() && m.lighting == LightingModel::Lit && env_map.is_none() && emissive_map.is_none() && !debug("unlit");
        self.use_program(if env_map.is_some() { 1 } else if receive { PROGRAM_RECEIVE } else { 0 });
        self.set_uv_st(UV_IDENTITY);
        // SAFETY: light env / texture state owned by the backend.
        unsafe {
            if let (true, Some(s)) = (receive, self.shadow.as_mut()) {
                c3d::C3D_TexBind(0, &mut *s.tex);
                // a small bias against acne (fraction of the depth range)
                c3d::C3D_TexShadowParams(false, 0.004);
                c3d::C3D_LightEnvShadowMode(&mut self.fixed.light_env, c3d::GPU_SHADOW_PRIMARY | c3d::GPU_SHADOW_SECONDARY);
                c3d::C3D_LightEnvShadowSel(&mut self.fixed.light_env, 0);
            } else {
                c3d::C3D_LightEnvShadowMode(&mut self.fixed.light_env, 0);
            }
        }
        self.set_procedural(m.procedural.as_ref());
        // SAFETY: texenv / light env / blend calls with valid pointers owned by the backend.
        unsafe {
            let tex = if debug("notex") { None } else { tex };
            // units: standard = base 0 / emissive 1; reflect = cube 0 / base 1; receive = shadow 0 / base 1
            let (base_unit, base_src) = if env_map.is_some() || receive { (1, ctru::GPU_TEXTURE1) } else { (0, ctru::GPU_TEXTURE0) };
            if let Some(t) = tex {
                c3d::C3D_TexBind(base_unit, t);
            }
            match (env_map, emissive_map) {
                (Some(c), _) => c3d::C3D_TexBind(0, c),
                (None, Some(e)) => c3d::C3D_TexBind(1, e),
                (None, None) if receive => {}
                (None, None) => self.unbind_texture(1),
            }
            if (env_map.is_some() || receive) && tex.is_none() {
                self.unbind_texture(1);
            }
            let env0 = c3d::C3D_GetTexEnv(0);
            c3d::C3D_TexEnvInit(env0);
            if m.procedural.is_some() {
                // the procedural texture replaces the base texture
                c3d::C3D_TexEnvSrc(env0, c3d::C3D_Both, ctru::GPU_TEXTURE3, ctru::GPU_PRIMARY_COLOR, 0);
                c3d::C3D_TexEnvFunc(env0, c3d::C3D_Both, ctru::GPU_MODULATE);
            } else if tex.is_some() {
                c3d::C3D_TexEnvSrc(env0, c3d::C3D_Both, base_src, ctru::GPU_PRIMARY_COLOR, 0);
                c3d::C3D_TexEnvFunc(env0, c3d::C3D_Both, ctru::GPU_MODULATE);
            } else {
                c3d::C3D_TexEnvSrc(env0, c3d::C3D_Both, ctru::GPU_PRIMARY_COLOR, 0, 0);
                c3d::C3D_TexEnvFunc(env0, c3d::C3D_Both, ctru::GPU_REPLACE);
            }
            let env1 = c3d::C3D_GetTexEnv(1);
            let env2 = c3d::C3D_GetTexEnv(2);
            c3d::C3D_TexEnvInit(env1);
            c3d::C3D_TexEnvInit(env2);
            let bc = m.base_color;
            if m.lighting == LightingModel::Lit && !debug("unlit") {
                // rgb × (ambient + diffuse) [material colors in the light env], alpha × base alpha
                c3d::C3D_TexEnvSrc(env1, c3d::C3D_RGB, ctru::GPU_PREVIOUS, ctru::GPU_FRAGMENT_PRIMARY_COLOR, 0);
                c3d::C3D_TexEnvFunc(env1, c3d::C3D_RGB, ctru::GPU_MODULATE);
                c3d::C3D_TexEnvSrc(env1, c3d::C3D_Alpha, ctru::GPU_PREVIOUS, ctru::GPU_CONSTANT, 0);
                c3d::C3D_TexEnvFunc(env1, c3d::C3D_Alpha, ctru::GPU_MODULATE);
                c3d::C3D_TexEnvColor(env1, abgr_u32(Color::rgba(1.0, 1.0, 1.0, bc.a)));
                // + specular
                c3d::C3D_TexEnvSrc(env2, c3d::C3D_RGB, ctru::GPU_PREVIOUS, ctru::GPU_FRAGMENT_SECONDARY_COLOR, 0);
                c3d::C3D_TexEnvFunc(env2, c3d::C3D_RGB, ctru::GPU_ADD);
                let s = m.specular;
                // an emissive map adds the emission in stage 3 instead
                let e = if emissive_map.is_some() { Color::BLACK } else { m.emissive };
                let toon = m.toon_steps > 0;
                let rim = m.rim_color;
                let has_rim = rim.r + rim.g + rim.b > 0.0;
                // C3D_Material colors are stored B, G, R (verified in Citra: a blue material showed orange)
                // toon: the banded diffuse comes through specular0 × D0(L·N) (diffuse itself can't
                // be shaped by a LUT); rim: specular1 × D1(N·V)
                let mat = c3d::C3D_Material {
                    ambient: [bc.b, bc.g, bc.r],
                    diffuse: if toon { [0.0; 3] } else { [bc.b, bc.g, bc.r] },
                    specular0: if toon { [bc.b, bc.g, bc.r] } else { [s.b, s.g, s.r] },
                    specular1: if has_rim { [rim.b, rim.g, rim.r] } else { [0.0; 3] },
                    emission: [e.b, e.g, e.r],
                };
                c3d::C3D_LightEnvBind(&mut self.fixed.light_env);
                c3d::C3D_LightEnvMaterial(&mut self.fixed.light_env, &mat);
                self.bind_specular_lut(m.shininess, m.toon_steps);
                self.bind_rim_lut(has_rim.then_some(m.rim_power.max(0.1)));
                if toon {
                    // (ambient + banded light) × texture: (primary + secondary) × previous
                    c3d::C3D_TexEnvSrc(env1, c3d::C3D_RGB, ctru::GPU_FRAGMENT_PRIMARY_COLOR, ctru::GPU_FRAGMENT_SECONDARY_COLOR, ctru::GPU_PREVIOUS);
                    c3d::C3D_TexEnvFunc(env1, c3d::C3D_RGB, ctru::GPU_ADD_MULTIPLY);
                    c3d::C3D_TexEnvInit(env2);
                }
            } else {
                c3d::C3D_LightEnvBind(std::ptr::null_mut());
                c3d::C3D_TexEnvSrc(env1, c3d::C3D_Both, ctru::GPU_PREVIOUS, ctru::GPU_CONSTANT, 0);
                c3d::C3D_TexEnvFunc(env1, c3d::C3D_Both, ctru::GPU_MODULATE);
                c3d::C3D_TexEnvColor(env1, abgr_u32(bc));
            }
            // stage 3: reflection (mix with the cube map) or emission (+ map × emissive color)
            let env3 = c3d::C3D_GetTexEnv(3);
            c3d::C3D_TexEnvInit(env3);
            if env_map.is_some() {
                let r = m.reflectivity.clamp(0.0, 1.0);
                c3d::C3D_TexEnvSrc(env3, c3d::C3D_RGB, ctru::GPU_TEXTURE0, ctru::GPU_PREVIOUS, ctru::GPU_CONSTANT);
                c3d::C3D_TexEnvFunc(env3, c3d::C3D_RGB, ctru::GPU_INTERPOLATE);
                c3d::C3D_TexEnvColor(env3, abgr_u32(Color::rgba(r, r, r, 1.0)));
            } else if emissive_map.is_some() {
                c3d::C3D_TexEnvSrc(env3, c3d::C3D_RGB, ctru::GPU_TEXTURE1, ctru::GPU_CONSTANT, ctru::GPU_PREVIOUS);
                c3d::C3D_TexEnvFunc(env3, c3d::C3D_RGB, ctru::GPU_MULTIPLY_ADD);
                c3d::C3D_TexEnvColor(env3, abgr_u32(m.emissive));
            }
            // stage 4: the view's color grading (exposure × tint; the rest of PostFx is desktop-only)
            let env4 = c3d::C3D_GetTexEnv(4);
            c3d::C3D_TexEnvInit(env4);
            if let Some(g) = self.view_grade {
                c3d::C3D_TexEnvSrc(env4, c3d::C3D_RGB, ctru::GPU_PREVIOUS, ctru::GPU_CONSTANT, 0);
                c3d::C3D_TexEnvFunc(env4, c3d::C3D_RGB, ctru::GPU_MODULATE);
                c3d::C3D_TexEnvColor(env4, abgr_u32(g));
            }
            // stage 5 and the combiner buffer: only Goat shader programs use them
            c3d::C3D_TexEnvInit(c3d::C3D_GetTexEnv(5));
            c3d::C3D_TexEnvBufUpdate(c3d::C3D_Both as i32, 0);
        }
        self.set_render_state(m);
    }

    /// A Goat shader compiled for the texture combiners (goat-shader pica.rs): its stages, its
    /// two textures, the fragment lighting with a white material when it is lit (the program
    /// multiplies the light itself), then the view's grading in the next free stage.
    fn set_tev_material(&mut self, m: &MaterialDesc, p: &goat_gfx::TevProgram) {
        use goat_gfx::{TevFunc, TevOperand, TevSource};
        self.use_program(0);
        self.set_uv_st(p.uv);
        // SAFETY: light env owned by the backend.
        unsafe { c3d::C3D_LightEnvShadowMode(&mut self.fixed.light_env, 0) };
        self.set_procedural(None);
        let tex = |i: usize, s: &mut Self| p.textures[i].and_then(|u| m.shader_textures.get(u).copied().flatten()).and_then(|t| s.gpu_textures.get_mut(&t.index())).map(|t| &mut *t.tex as *mut c3d::C3D_Tex);
        let (t0, t1) = (tex(0, self), tex(1, self));
        let src = |s: TevSource| match s {
            TevSource::Vertex => ctru::GPU_PRIMARY_COLOR,
            TevSource::LightDiffuse => ctru::GPU_FRAGMENT_PRIMARY_COLOR,
            TevSource::LightSpecular => ctru::GPU_FRAGMENT_SECONDARY_COLOR,
            TevSource::Texture0 => ctru::GPU_TEXTURE0,
            TevSource::Texture1 => ctru::GPU_TEXTURE1,
            TevSource::Constant => ctru::GPU_CONSTANT,
            TevSource::Previous => ctru::GPU_PREVIOUS,
            TevSource::Buffer => ctru::GPU_PREVIOUS_BUFFER,
        };
        let op_rgb = |o: TevOperand| match o {
            TevOperand::Color => ctru::GPU_TEVOP_RGB_SRC_COLOR,
            TevOperand::OneMinusColor => ctru::GPU_TEVOP_RGB_ONE_MINUS_SRC_COLOR,
            TevOperand::Alpha => ctru::GPU_TEVOP_RGB_SRC_ALPHA,
            TevOperand::OneMinusAlpha => ctru::GPU_TEVOP_RGB_ONE_MINUS_SRC_ALPHA,
            TevOperand::R => ctru::GPU_TEVOP_RGB_SRC_R,
            TevOperand::OneMinusR => ctru::GPU_TEVOP_RGB_ONE_MINUS_SRC_R,
            TevOperand::G => ctru::GPU_TEVOP_RGB_SRC_G,
            TevOperand::OneMinusG => ctru::GPU_TEVOP_RGB_ONE_MINUS_SRC_G,
            TevOperand::B => ctru::GPU_TEVOP_RGB_SRC_B,
            TevOperand::OneMinusB => ctru::GPU_TEVOP_RGB_ONE_MINUS_SRC_B,
        };
        let op_a = |o: TevOperand| match o {
            TevOperand::Color | TevOperand::Alpha => ctru::GPU_TEVOP_A_SRC_ALPHA,
            TevOperand::OneMinusColor | TevOperand::OneMinusAlpha => ctru::GPU_TEVOP_A_ONE_MINUS_SRC_ALPHA,
            TevOperand::R => ctru::GPU_TEVOP_A_SRC_R,
            TevOperand::OneMinusR => ctru::GPU_TEVOP_A_ONE_MINUS_SRC_R,
            TevOperand::G => ctru::GPU_TEVOP_A_SRC_G,
            TevOperand::OneMinusG => ctru::GPU_TEVOP_A_ONE_MINUS_SRC_G,
            TevOperand::B => ctru::GPU_TEVOP_A_SRC_B,
            TevOperand::OneMinusB => ctru::GPU_TEVOP_A_ONE_MINUS_SRC_B,
        };
        let func = |f: TevFunc| match f {
            TevFunc::Replace => ctru::GPU_REPLACE,
            TevFunc::Modulate => ctru::GPU_MODULATE,
            TevFunc::Add => ctru::GPU_ADD,
            TevFunc::Interpolate => ctru::GPU_INTERPOLATE,
            TevFunc::Subtract => ctru::GPU_SUBTRACT,
            TevFunc::MultiplyAdd => ctru::GPU_MULTIPLY_ADD,
        };
        let scale = |k: u8| match k {
            2 => ctru::GPU_TEVSCALE_2,
            4 => ctru::GPU_TEVSCALE_4,
            _ => ctru::GPU_TEVSCALE_1,
        };
        // SAFETY: texenv / light env calls with pointers owned by the backend.
        unsafe {
            c3d::C3D_TexBind(0, t0.unwrap_or(std::ptr::null_mut()));
            c3d::C3D_TexBind(1, t1.unwrap_or(std::ptr::null_mut()));
            if p.lit && !debug("unlit") {
                let mat = c3d::C3D_Material { ambient: [1.0; 3], diffuse: [1.0; 3], specular0: [1.0; 3], specular1: [0.0; 3], emission: [0.0; 3] };
                c3d::C3D_LightEnvBind(&mut self.fixed.light_env);
                c3d::C3D_LightEnvMaterial(&mut self.fixed.light_env, &mat);
                self.bind_specular_lut(32.0, 0);
                self.bind_rim_lut(None);
            } else {
                c3d::C3D_LightEnvBind(std::ptr::null_mut());
            }
            let (mut buf_rgb, mut buf_a) = (0, 0);
            for i in 0..6 {
                let env = c3d::C3D_GetTexEnv(i as i32);
                c3d::C3D_TexEnvInit(env);
                if let Some(s) = p.stages.get(i) {
                    let (r, a) = (&s.rgb, &s.alpha);
                    c3d::C3D_TexEnvSrc(env, c3d::C3D_RGB, src(r.sources[0]), src(r.sources[1]), src(r.sources[2]));
                    c3d::C3D_TexEnvSrc(env, c3d::C3D_Alpha, src(a.sources[0]), src(a.sources[1]), src(a.sources[2]));
                    c3d::C3D_TexEnvOpRgb(env, op_rgb(r.operands[0]), op_rgb(r.operands[1]), op_rgb(r.operands[2]));
                    c3d::C3D_TexEnvOpAlpha(env, op_a(a.operands[0]), op_a(a.operands[1]), op_a(a.operands[2]));
                    c3d::C3D_TexEnvFunc(env, c3d::C3D_RGB, func(r.func));
                    c3d::C3D_TexEnvFunc(env, c3d::C3D_Alpha, func(a.func));
                    c3d::C3D_TexEnvScale(env, c3d::C3D_RGB as i32, scale(r.scale));
                    c3d::C3D_TexEnvScale(env, c3d::C3D_Alpha as i32, scale(a.scale));
                    let k = s.constant;
                    c3d::C3D_TexEnvColor(env, abgr_u32(Color::rgba(k[0], k[1], k[2], k[3])));
                    if i < 4 {
                        buf_rgb |= (s.buffer_rgb as i32) << i;
                        buf_a |= (s.buffer_alpha as i32) << i;
                    }
                } else if i == p.stages.len() {
                    if let Some(g) = self.view_grade {
                        c3d::C3D_TexEnvSrc(env, c3d::C3D_RGB, ctru::GPU_PREVIOUS, ctru::GPU_CONSTANT, 0);
                        c3d::C3D_TexEnvFunc(env, c3d::C3D_RGB, ctru::GPU_MODULATE);
                        c3d::C3D_TexEnvColor(env, abgr_u32(g));
                    }
                }
            }
            c3d::C3D_TexEnvBufUpdate(c3d::C3D_RGB as i32, buf_rgb);
            c3d::C3D_TexEnvBufUpdate(c3d::C3D_Alpha as i32, buf_a);
        }
    }

    /// Cull / depth / blend / alpha test / fog of a material.
    fn set_render_state(&mut self, m: &MaterialDesc) {
        // SAFETY: fixed-function state.
        unsafe {
            c3d::C3D_CullFace(match m.cull {
                CullMode::None => ctru::GPU_CULL_NONE,
                CullMode::Back => ctru::GPU_CULL_BACK_CCW,
                CullMode::Front => ctru::GPU_CULL_FRONT_CCW,
            });
            c3d::C3D_DepthTest(true, if m.depth_test { ctru::GPU_GREATER } else { ctru::GPU_ALWAYS }, if m.depth_write { ctru::GPU_WRITE_ALL } else { ctru::GPU_WRITE_COLOR });
            let (src, dst) = match m.blend {
                BlendMode::Opaque => (ctru::GPU_ONE, ctru::GPU_ZERO),
                BlendMode::Alpha => (ctru::GPU_SRC_ALPHA, ctru::GPU_ONE_MINUS_SRC_ALPHA),
                BlendMode::Additive => (ctru::GPU_SRC_ALPHA, ctru::GPU_ONE),
            };
            c3d::C3D_AlphaBlend(ctru::GPU_BLEND_ADD, ctru::GPU_BLEND_ADD, src, dst, src, dst);
            // alpha test (cut-outs); dithering isn't available here
            if m.alpha_cutoff > 0.0 {
                c3d::C3D_AlphaTest(true, ctru::GPU_GEQUAL, (m.alpha_cutoff.clamp(0.0, 1.0) * 255.0) as i32);
            } else {
                c3d::C3D_AlphaTest(false, ctru::GPU_ALWAYS, 0);
            }
            let fog = if self.view_fog && m.fog { ctru::GPU_FOG } else { ctru::GPU_NO_FOG };
            c3d::C3D_FogGasMode(fog, ctru::GPU_PLAIN_DENSITY, false);
        }
    }

    /// Fills the current viewport with the far depth and, with a color, that color
    /// (partial-viewport and depth-only clears; full color clears use C3D_RenderTargetClear).
    fn clear_viewport(&mut self, color: Option<Color>) {
        self.use_program(0);
        self.set_procedural(None);
        let Some(quad) = &self.clear_quad else { return };
        let (vbo, ibo, count) = (quad.vertices(), quad.indices(), quad.index_count);
        let identity = to_c3d(Mat4::IDENTITY);
        // SAFETY: state + one draw of the backend's own quad (clip space, at the far plane).
        unsafe {
            c3d::C3D_LightEnvBind(std::ptr::null_mut());
            c3d::C3D_FogGasMode(ctru::GPU_NO_FOG, ctru::GPU_PLAIN_DENSITY, false);
            let env0 = c3d::C3D_GetTexEnv(0);
            c3d::C3D_TexEnvInit(env0);
            c3d::C3D_TexEnvSrc(env0, c3d::C3D_Both, ctru::GPU_CONSTANT, 0, 0);
            c3d::C3D_TexEnvFunc(env0, c3d::C3D_Both, ctru::GPU_REPLACE);
            c3d::C3D_TexEnvColor(env0, abgr_u32(color.unwrap_or(Color::BLACK)));
            c3d::C3D_TexEnvInit(c3d::C3D_GetTexEnv(1));
            c3d::C3D_TexEnvInit(c3d::C3D_GetTexEnv(2));
            c3d::C3D_TexEnvInit(c3d::C3D_GetTexEnv(3));
            c3d::C3D_TexEnvInit(c3d::C3D_GetTexEnv(4));
            c3d::C3D_AlphaTest(false, ctru::GPU_ALWAYS, 0);
            c3d::C3D_CullFace(ctru::GPU_CULL_NONE);
            c3d::C3D_DepthTest(true, ctru::GPU_ALWAYS, if color.is_some() { ctru::GPU_WRITE_ALL } else { ctru::GPU_WRITE_DEPTH });
            c3d::C3D_DepthMap(true, -1.0, 0.0);
            c3d::C3D_AlphaBlend(ctru::GPU_BLEND_ADD, ctru::GPU_BLEND_ADD, ctru::GPU_ONE, ctru::GPU_ZERO, ctru::GPU_ONE, ctru::GPU_ZERO);
            c3d::C3D_FVUnifMtx4x4(ctru::GPU_VERTEX_SHADER, self.loc_projection, &identity);
            c3d::C3D_FVUnifMtx4x4(ctru::GPU_VERTEX_SHADER, self.loc_model_view, &identity);
            let buf = c3d::C3D_GetBufInfo();
            c3d::BufInfo_Init(buf);
            c3d::BufInfo_Add(buf, vbo, Vertex::SIZE as isize, 4, 0x3210);
            c3d::C3D_DrawElements(ctru::GPU_TRIANGLES, count, c3d::C3D_UNSIGNED_SHORT as i32, ibo);
        }
    }

    /// Draws one view into a target (already selected with C3D_FrameDrawOn).
    fn draw_view(&mut self, target: *mut c3d::C3D_RenderTarget, v: &RenderView, eye: f32, slider: f32, stats: &mut FrameStats) {
        // Viewport: the screens are rotated 90°: screen y (down) runs along the target's GPU x
        // reversed, screen x (right) along its GPU y reversed (verified in Citra).
        let (tw, th) = target_size(target);
        let vp = v.viewport;
        let full = vp == Viewport::FULL;
        if !full && self.into_texture {
            // a texture isn't rotated: x / y as they are
            // SAFETY: viewport inside the current target.
            unsafe { c3d::C3D_SetViewport((vp.x * tw).round() as u32, (vp.y * th).round() as u32, (vp.width * tw).round().max(1.0) as u32, (vp.height * th).round().max(1.0) as u32) };
        } else if !full && !debug("vpfull") {
            let x = ((1.0 - (vp.y + vp.height)) * tw).round().max(0.0);
            let y = ((1.0 - (vp.x + vp.width)) * th).round().max(0.0);
            let w = (vp.height * tw).round().max(1.0);
            let h = (vp.width * th).round().max(1.0);
            if debug("logvp") {
                static ONCE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                if ONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 2 {
                    goat_core::info!("viewport target {tw}x{th} -> ({x}, {y}, {w}, {h})");
                }
            }
            // SAFETY: viewport inside the current target.
            unsafe { c3d::C3D_SetViewport(x as u32, y as u32, w as u32, h as u32) };
        }
        if !full && v.clear.is_some() {
            self.clear_viewport(v.clear);
        } else if v.clear.is_none() && v.clear_depth {
            // a quad, in command order (a memory-fill clear mid-frame could run before the draws
            // of earlier views that are still queued)
            self.clear_viewport(None);
        }
        let (dw, dh) = if self.into_texture { (tw, th) } else if v.display == Display::Main { (400.0, 240.0) } else { (320.0, 240.0) };
        let aspect = (dw * vp.width) / (dh * vp.height).max(1e-4);
        let proj = self.projection(v.projection, aspect, eye, slider);
        // Depth: perspective views use the PICA's W-buffer, depth = clip z × scale (no division
        // by w). The tilt projections give clip z = -near at the near plane and 0 at the far one,
        // so scale -1/near makes depth linear in view z: 1 near, 0 far. That is uniform precision
        // (24 bits over the whole range) and a linear fog LUT. Orthographic: clip z is already
        // linear in [-1, 0].
        // SAFETY: fixed-function state.
        unsafe {
            match v.projection {
                Projection::Perspective { near, .. } if !debug("zbuffer") => c3d::C3D_DepthMap(false, -1.0 / near.max(1e-4), 0.0),
                _ => c3d::C3D_DepthMap(true, -1.0, 0.0),
            }
        }
        // the view's matrices go to the bound program now and to the other one when it is used
        let inv = v.view.inverse();
        self.view_inv = [0, 1, 2].map(|r| [inv.x_axis[r], inv.y_axis[r], inv.z_axis[r], 0.0]);
        self.view_proj = Some(proj);
        let k = self.bound_program;
        self.bound_program = usize::MAX;
        // the shadow map follows this view's shadow light (when it is the one the pass used)
        self.view_shadow = match (v.lighting.shadow, self.shadow_dir) {
            (Some(i), Some(d)) => v.lighting.lights.get(i).filter(|l| l.kind == LightKind::Directional && (l.direction.normalize_or(Vec3::NEG_Y) - d).length() < 1e-3).map(|_| i),
            _ => None,
        };
        self.use_program(k);
        self.set_lights(v);
        self.probe_light = None;
        self.set_fog(v);
        self.view_time = v.time;
        self.view_grade = v.post.map(|p| Color::rgba((p.tint.r * p.exposure).min(1.0), (p.tint.g * p.exposure).min(1.0), (p.tint.b * p.exposure).min(1.0), 1.0));

        let order = v.draw_order(|m| self.materials.get(m).map(|m| m.blend));
        let mut last_material = None;
        for i in order {
            let it = &v.items[i];
            let Some(mesh) = self.gpu_meshes.get(&it.mesh.index()) else { continue };
            let (vbo, ibo, count) = (mesh.vertices(), mesh.indices(), mesh.index_count);
            if count == 0 {
                continue;
            }
            // a reflection probe's cube map replaces the env map of reflective materials
            let env = it.reflection.filter(|r| self.cubes.contains(&r.index()) && self.materials.get(it.material).is_some_and(|m| m.reflectivity > 0.0 && m.tev.is_none()));
            let tint = it.color.to_array().map(f32::to_bits);
            if last_material != Some((it.material, env, tint)) {
                let Some(mut m) = self.materials.get(it.material).cloned() else { continue };
                if env.is_some() {
                    m.env_map = env;
                }
                // MaterialPropertyBlock / per-instance color: multiplies the material's color
                if it.color != Color::WHITE {
                    let (b, c) = (m.base_color, it.color);
                    m.base_color = Color::rgba(b.r * c.r, b.g * c.g, b.b * c.b, b.a * c.a);
                }
                let tex = m.texture.and_then(|t| self.gpu_textures.get_mut(&t.index())).map(|t| &mut *t.tex as *mut c3d::C3D_Tex);
                self.set_material(&m, tex);
                last_material = Some((it.material, env, tint));
            }
            self.set_probe_light(v, it.light_probe);
            let mv = to_c3d(v.view * it.model);
            self.upload_light_mtx(&it.model);
            // SAFETY: buffers live in linear memory owned by `gpu_meshes`.
            unsafe {
                c3d::C3D_FVUnifMtx4x4(ctru::GPU_VERTEX_SHADER, self.loc_model_view, &mv);
                let buf = c3d::C3D_GetBufInfo();
                c3d::BufInfo_Init(buf);
                c3d::BufInfo_Add(buf, vbo, Vertex::SIZE as isize, 4, 0x3210);
                if !debug("nodrawcall") {
                    c3d::C3D_DrawElements(ctru::GPU_TRIANGLES, count, c3d::C3D_UNSIGNED_SHORT as i32, ibo);
                }
            }
            stats.draw_calls += 1;
            stats.triangles += count as u32 / 3;
            // toon outline: back faces pushed out along the normals, flat color
            let outline = self.materials.get(it.material).filter(|m| m.outline_width > 0.0).map(|m| (m.outline_width, m.outline_color, m.fog));
            if let Some((width, color, fog)) = outline {
                self.use_program(2);
                // SAFETY: state + one more draw of the same buffers.
                unsafe {
                    c3d::C3D_FVUnifSet(ctru::GPU_VERTEX_SHADER, self.locs[2][3], width, 0.0, 0.0, 0.0);
                    c3d::C3D_FVUnifMtx4x4(ctru::GPU_VERTEX_SHADER, self.loc_model_view, &mv);
                    c3d::C3D_LightEnvBind(std::ptr::null_mut());
                    let env0 = c3d::C3D_GetTexEnv(0);
                    c3d::C3D_TexEnvInit(env0);
                    c3d::C3D_TexEnvSrc(env0, c3d::C3D_Both, ctru::GPU_CONSTANT, 0, 0);
                    c3d::C3D_TexEnvFunc(env0, c3d::C3D_Both, ctru::GPU_REPLACE);
                    c3d::C3D_TexEnvColor(env0, abgr_u32(Color::rgba(color.r, color.g, color.b, 1.0)));
                    for k in 1..5 {
                        c3d::C3D_TexEnvInit(c3d::C3D_GetTexEnv(k));
                    }
                    c3d::C3D_AlphaTest(false, ctru::GPU_ALWAYS, 0);
                    c3d::C3D_CullFace(ctru::GPU_CULL_FRONT_CCW);
                    c3d::C3D_DepthTest(true, ctru::GPU_GREATER, ctru::GPU_WRITE_ALL);
                    c3d::C3D_AlphaBlend(ctru::GPU_BLEND_ADD, ctru::GPU_BLEND_ADD, ctru::GPU_ONE, ctru::GPU_ZERO, ctru::GPU_ONE, ctru::GPU_ZERO);
                    c3d::C3D_FogGasMode(if self.view_fog && fog { ctru::GPU_FOG } else { ctru::GPU_NO_FOG }, ctru::GPU_PLAIN_DENSITY, false);
                    let buf = c3d::C3D_GetBufInfo();
                    c3d::BufInfo_Init(buf);
                    c3d::BufInfo_Add(buf, vbo, Vertex::SIZE as isize, 4, 0x3210);
                    c3d::C3D_DrawElements(ctru::GPU_TRIANGLES, count, c3d::C3D_UNSIGNED_SHORT as i32, ibo);
                }
                stats.draw_calls += 1;
                // the next item sets its material again
                last_material = None;
            }
            // the material's other passes (multi-pass shaders): the same buffers again
            let mut next = self.materials.get(it.material).and_then(|m| m.next_pass);
            let mut passes = 1;
            while let Some(pm) = next.filter(|n| passes < 4 && *n != it.material) {
                passes += 1;
                let Some(m) = self.materials.get(pm).cloned() else { break };
                let tex = m.texture.and_then(|t| self.gpu_textures.get_mut(&t.index())).map(|t| &mut *t.tex as *mut c3d::C3D_Tex);
                self.set_material(&m, tex);
                last_material = None;
                self.upload_light_mtx(&it.model);
                // SAFETY: one more draw of the same buffers (the program may have changed).
                unsafe {
                    c3d::C3D_FVUnifMtx4x4(ctru::GPU_VERTEX_SHADER, self.loc_model_view, &mv);
                    let buf = c3d::C3D_GetBufInfo();
                    c3d::BufInfo_Init(buf);
                    c3d::BufInfo_Add(buf, vbo, Vertex::SIZE as isize, 4, 0x3210);
                    c3d::C3D_DrawElements(ctru::GPU_TRIANGLES, count, c3d::C3D_UNSIGNED_SHORT as i32, ibo);
                }
                stats.draw_calls += 1;
                stats.triangles += count as u32 / 3;
                next = m.next_pass;
            }
        }
        if !full {
            // SAFETY: back to the whole target for the next view.
            unsafe { c3d::C3D_SetViewport(0, 0, tw as u32, th as u32) };
        }
    }
}

impl Drop for Citro3dBackend {
    fn drop(&mut self) {
        self.gpu_meshes.clear();
        self.gpu_textures.clear();
        self.clear_quad = None;
        // SAFETY: tear down in reverse order of creation.
        unsafe {
            if let Some(mut s) = self.shadow.take() {
                c3d::C3D_RenderTargetDelete(s.target);
                c3d::C3D_TexDelete(&mut *s.tex);
            }
            for t in [self.top_ssaa, self.top_left, self.top_right, self.bottom] {
                if !t.is_null() {
                    c3d::C3D_RenderTargetDelete(t);
                }
            }
            for k in 0..PROGRAMS {
                let _ = ctru::shaderProgramFree(&mut self.fixed.programs[k] as *mut c3d::shaderProgram_s as *mut ctru::shaderProgram_s);
                ctru::DVLB_Free(self.dvlbs[k]);
            }
            c3d::C3D_Fini();
        }
    }
}

impl GraphicsBackend for Citro3dBackend {
    fn name(&self) -> &str {
        "citro3d/PICA200"
    }

    fn displays(&self) -> Vec<DisplayInfo> {
        vec![
            DisplayInfo { display: Display::Main, width: 400, height: 240, stereo: true },
            DisplayInfo { display: Display::Secondary, width: 320, height: 240, stereo: false },
        ]
    }

    fn create_mesh(&mut self, mesh: &MeshData) -> Result<MeshId, GfxError> {
        let gpu = mesh_upload(mesh)?;
        let id = self.meshes.insert(MeshData::default());
        self.gpu_meshes.insert(id.index(), gpu);
        Ok(id)
    }

    fn update_texture(&mut self, id: TextureId, t: &TextureData) -> Result<(), GfxError> {
        let meta = self.textures.get(id).ok_or_else(|| GfxError::Backend("update_texture: unknown texture".into()))?;
        if (meta.width, meta.height) != (t.width, t.height) || t.rgba.len() != (t.width * t.height * 4) as usize {
            return Err(GfxError::Unsupported("update_texture: size changed".into()));
        }
        let format = meta.format;
        let Some(gpu) = self.gpu_textures.get_mut(&id.index()) else { return Ok(()) };
        let tiled = tile_texture(t.width, t.height, &t.rgba, format);
        // SAFETY: the texture was created with this size / format; level 0 is overwritten and flushed.
        unsafe {
            c3d::C3D_TexLoadImage(&mut *gpu.tex, tiled.as_ptr() as *const c_void, ctru::GPU_TEXFACE_2D, 0);
            c3d::C3D_TexFlush(&mut *gpu.tex);
        }
        Ok(())
    }

    fn update_mesh(&mut self, id: MeshId, data: &MeshData) -> Result<(), GfxError> {
        if self.meshes.get(id).is_none() {
            return Err(GfxError::Backend("update_mesh: unknown mesh".into()));
        }
        let Some(mesh) = self.gpu_meshes.get_mut(&id.index()) else { return Ok(()) };
        let (vb, ib) = (data.vertex_bytes(), index_bytes(data));
        let (mut v, mut i) = match mesh.back.take() {
            Some(b) => b,
            None => (LinearBuf::alloc(vb.len().next_power_of_two())?, LinearBuf::alloc(ib.len().next_power_of_two())?),
        };
        v.write(vb)?;
        i.write(ib)?;
        let front_v = std::mem::replace(&mut mesh.vertices, v);
        let front_i = std::mem::replace(&mut mesh.indices, i);
        mesh.back = Some((front_v, front_i));
        mesh.index_count = data.indices.len() as i32;
        mesh.bounds = data.bounds();
        Ok(())
    }

    fn mesh_bounds(&self, id: MeshId) -> Option<Bounds> {
        self.meshes.get(id)?;
        self.gpu_meshes.get(&id.index()).map(|m| m.bounds)
    }

    fn create_texture(&mut self, t: &TextureData) -> Result<TextureId, GfxError> {
        let ok_size = |s: u32| s.is_power_of_two() && (8..=1024).contains(&s);
        if !ok_size(t.width) || !ok_size(t.height) {
            return Err(GfxError::Unsupported(format!("3DS textures must be powers of two from 8 to 1024 ({}x{})", t.width, t.height)));
        }
        let gpu_format = match t.format {
            TextureFormat::Rgba8 => ctru::GPU_RGBA8,
            TextureFormat::Rgb565 => ctru::GPU_RGB565,
            TextureFormat::Rgba4 => ctru::GPU_RGBA4,
            TextureFormat::Rgba5551 => ctru::GPU_RGBA5551,
            TextureFormat::Etc1 => ctru::GPU_ETC1,
            TextureFormat::Etc1A4 => ctru::GPU_ETC1A4,
        };
        // precompressed ETC1 (KTX, builds): swizzled as is, no encoding on the console
        let pre = t.compressed.as_ref().filter(|c| matches!((t.format, c.alpha), (TextureFormat::Etc1, false) | (TextureFormat::Etc1A4, true)));
        // the PICA stops at 8×8
        let levels: Vec<(u32, u32, Vec<u8>)> = match pre {
            Some(c) => c
                .levels
                .iter()
                .enumerate()
                .map(|(i, blocks)| (t.width >> i, t.height >> i, blocks))
                .take_while(|(w, h, _)| *w >= 8 && *h >= 8)
                .take(if t.mipmaps { usize::MAX } else { 1 })
                .map(|(w, h, blocks)| (w, h, crate::etc1::from_blocks(w, h, blocks, c.alpha)))
                .collect(),
            None => t.mip_chain(8).into_iter().map(|(w, h, rgba)| (w, h, tile_texture(w, h, &rgba, t.format))).collect(),
        };
        // SAFETY: C3D_Tex boxed (stable address); each level uploaded once.
        unsafe {
            let mut tex: Box<c3d::C3D_Tex> = Box::new(std::mem::zeroed());
            let mut params: c3d::C3D_TexInitParams = std::mem::zeroed();
            params.width = t.width as u16;
            params.height = t.height as u16;
            params.set_maxLevel((levels.len() - 1) as u8);
            params.set_format(gpu_format);
            params.set_type(ctru::GPU_TEX_2D);
            params.set_onVram(false);
            if !c3d::C3D_TexInitWithParams(&mut *tex, std::ptr::null_mut(), params) {
                return Err(GfxError::OutOfMemory("texture".into()));
            }
            for (level, (_, _, tiled)) in levels.iter().enumerate() {
                c3d::C3D_TexLoadImage(&mut *tex, tiled.as_ptr() as *const c_void, ctru::GPU_TEXFACE_2D, level as i32);
            }
            c3d::C3D_TexFlush(&mut *tex);
            let f = if t.filter == Filter::Linear { ctru::GPU_LINEAR } else { ctru::GPU_NEAREST };
            c3d::C3D_TexSetFilter(&mut *tex, f, f);
            if levels.len() > 1 {
                c3d::C3D_TexSetFilterMipmap(&mut *tex, ctru::GPU_LINEAR);
            }
            let w = if t.wrap == Wrap::Repeat { ctru::GPU_REPEAT } else { ctru::GPU_CLAMP_TO_EDGE };
            c3d::C3D_TexSetWrap(&mut *tex, w, w);
            let id = self.textures.insert(t.clone_meta());
            self.gpu_textures.insert(id.index(), Texture { tex, cube: None, target: std::ptr::null_mut() });
            Ok(id)
        }
    }

    /// A VRAM texture + its render target. Sizes become powers of two (8..512, the PICA's
    /// rule); VRAM is shared with the screens' targets, so keep them small (128–256).
    fn create_render_target(&mut self, desc: &goat_gfx::RenderTargetDesc) -> Result<TextureId, GfxError> {
        let size = |s: u32| s.clamp(8, 512).next_power_of_two().min(512);
        let (w, h) = (size(desc.width), size(desc.height));
        // VRAM is mostly the screens' targets: color in VRAM if it fits, else in linear FCRAM (the
        // PICA renders there too, a little slower); 16-bit depth; smaller sizes as a last resort
        let depth = if desc.depth { c3d::C3D_DEPTHTYPE { __i: ctru::GPU_RB_DEPTH16 as i32 } } else { c3d::C3D_DEPTHTYPE { __i: -1 } };
        let mut sizes = vec![(w, h)];
        let (mut sw, mut sh) = (w, h);
        while sw > 32 && sh > 32 {
            sw /= 2;
            sh /= 2;
            sizes.push((sw, sh));
        }
        // SAFETY: C3D_Tex boxed (stable address); the target is deleted with the texture.
        unsafe {
            for (w, h) in sizes {
                for vram in [true, false] {
                    let mut tex: Box<c3d::C3D_Tex> = Box::new(std::mem::zeroed());
                    let ok = if vram { c3d::C3D_TexInitVRAM(&mut *tex, w as u16, h as u16, ctru::GPU_RGBA8) } else { c3d::C3D_TexInit(&mut *tex, w as u16, h as u16, ctru::GPU_RGBA8) };
                    if !ok {
                        continue;
                    }
                    let target = c3d::C3D_RenderTargetCreateFromTex(&mut *tex, ctru::GPU_TEXFACE_2D, 0, depth);
                    if target.is_null() {
                        c3d::C3D_TexDelete(&mut *tex);
                        continue;
                    }
                    if (w, h) != (desc.width, desc.height) || !vram {
                        goat_core::info!("render texture {}×{} → {w}×{h}{}", desc.width, desc.height, if vram { "" } else { " (color in FCRAM)" });
                    }
                    return Ok(self.finish_render_target(tex, target, w, h));
                }
            }
        }
        Err(GfxError::OutOfMemory(format!("render texture {w}×{h}: no VRAM for its depth buffer")))
    }

    fn texture_size(&self, id: TextureId) -> Option<(u32, u32)> {
        self.textures.get(id).map(|t| (t.width, t.height))
    }

    fn create_cubemap(&mut self, faces: &[TextureData; 6]) -> Result<TextureId, GfxError> {
        let n = faces[0].width;
        if !(n.is_power_of_two() && (8..=1024).contains(&n)) || faces.iter().any(|f| f.width != n || f.height != n) {
            return Err(GfxError::Unsupported(format!("3DS cube maps: 6 square power-of-two faces of one size ({n})")));
        }
        // SAFETY: C3D_Tex and its face table boxed (stable addresses); each face uploaded once.
        unsafe {
            let mut tex: Box<c3d::C3D_Tex> = Box::new(std::mem::zeroed());
            let mut cube: Box<c3d::C3D_TexCube> = Box::new(std::mem::zeroed());
            if !c3d::C3D_TexInitCube(&mut *tex, &mut *cube, n as u16, n as u16, ctru::GPU_RGBA8) {
                return Err(GfxError::OutOfMemory("cube map".into()));
            }
            for (i, f) in faces.iter().enumerate() {
                let tiled = tile_texture(n, n, &f.rgba, TextureFormat::Rgba8);
                c3d::C3D_TexLoadImage(&mut *tex, tiled.as_ptr() as *const c_void, ctru::GPU_POSITIVE_X + i as ctru::GPU_TEXFACE, 0);
            }
            c3d::C3D_TexFlush(&mut *tex);
            c3d::C3D_TexSetFilter(&mut *tex, ctru::GPU_LINEAR, ctru::GPU_LINEAR);
            c3d::C3D_TexSetWrap(&mut *tex, ctru::GPU_CLAMP_TO_EDGE, ctru::GPU_CLAMP_TO_EDGE);
            let id = self.textures.insert(faces[0].clone_meta());
            self.gpu_textures.insert(id.index(), Texture { tex, cube: Some(cube), target: std::ptr::null_mut() });
            self.cubes.insert(id.index());
            Ok(id)
        }
    }

    fn create_material(&mut self, mat: &MaterialDesc) -> Result<MaterialId, GfxError> {
        Ok(self.materials.insert(mat.clone()))
    }

    fn material_desc(&self, id: MaterialId) -> Option<MaterialDesc> {
        self.materials.get(id).cloned()
    }

    fn update_material(&mut self, id: MaterialId, mat: &MaterialDesc) {
        if let Some(m) = self.materials.get_mut(id) {
            *m = mat.clone();
        }
    }

    fn destroy_mesh(&mut self, id: MeshId) {
        if self.meshes.remove(id).is_some() {
            self.gpu_meshes.remove(&id.index());
        }
    }
    fn destroy_texture(&mut self, id: TextureId) {
        if self.textures.remove(id).is_some() {
            self.gpu_textures.remove(&id.index());
        }
    }
    fn destroy_material(&mut self, id: MaterialId) {
        self.materials.remove(id);
    }

    fn render(&mut self, views: &[RenderView]) -> FrameStats {
        let mut stats = FrameStats::default();
        // SAFETY: frame / target calls on targets owned by the backend.
        let slider = unsafe { ctru::osGet3DSliderState() };
        let stereo = slider > 0.0 || debug("nossaa");
        if stereo != self.stereo {
            self.stereo = stereo;
            // SAFETY: gfx is initialised by the platform.
            unsafe {
                ctru::gfxSet3D(stereo);
                if stereo {
                    c3d::C3D_RenderTargetDetachOutput(self.top_ssaa);
                    let flags = if self.eyes_aa { TRANSFER_FLAGS_AA_X } else { TRANSFER_FLAGS };
                    c3d::C3D_RenderTargetSetOutput(self.top_left, ctru::GFX_TOP, ctru::GFX_LEFT, flags);
                    c3d::C3D_RenderTargetSetOutput(self.top_right, ctru::GFX_TOP, ctru::GFX_RIGHT, flags);
                } else {
                    c3d::C3D_RenderTargetDetachOutput(self.top_left);
                    c3d::C3D_RenderTargetDetachOutput(self.top_right);
                    c3d::C3D_RenderTargetSetOutput(self.top_ssaa, ctru::GFX_TOP, ctru::GFX_LEFT, TRANSFER_FLAGS_SSAA);
                }
            }
        }
        // SAFETY: one frame of citro3d commands.
        unsafe {
            c3d::C3D_FrameBegin(c3d::C3D_FRAME_SYNCDRAW as u8);
        }
        self.bound_program = usize::MAX;
        self.view_proj = None;
        self.shadow_pass(views, &mut stats);
        self.use_program(0);
        let trace = debug("logstereo") && self.stereo;
        if trace {
            goat_core::info!("stereo frame: begin ok");
        }
        // cameras drawing into render textures first (their textures are sampled by the screens)
        for v in views.iter().filter(|v| v.target.is_some()) {
            let Some(target) = v.target.and_then(|t| self.gpu_textures.get(&t.index())).map(|t| t.target).filter(|t| !t.is_null()) else { continue };
            // SAFETY: a render target owned by one of our textures.
            unsafe {
                if let (Some(c), true) = (v.clear, v.viewport == Viewport::FULL) {
                    c3d::C3D_RenderTargetClear(target, c3d::C3D_CLEAR_ALL, rgba_u32(c), 0);
                }
                c3d::C3D_FrameDrawOn(target);
            }
            self.into_texture = true;
            self.draw_view(target, v, 0.0, slider, &mut stats);
            self.into_texture = false;
        }
        let passes: Vec<(*mut c3d::C3D_RenderTarget, Display, f32)> = if self.stereo {
            vec![(self.top_left, Display::Main, -1.0), (self.top_right, Display::Main, 1.0), (self.bottom, Display::Secondary, 0.0)]
        } else {
            vec![(self.top_ssaa, Display::Main, 0.0), (self.bottom, Display::Secondary, 0.0)]
        };
        for (target, display, eye) in passes {
            let full_clear = |v: &RenderView| v.clear.is_some() && v.viewport == Viewport::FULL;
            if !views.iter().any(|v| v.target.is_none() && v.display == display && full_clear(v)) {
                // no camera clears this whole display: start from black so stale frames don't stay
                // SAFETY: target is valid.
                unsafe { c3d::C3D_RenderTargetClear(target, c3d::C3D_CLEAR_ALL, 0x000000ff, 0) };
            }
            for v in views.iter().filter(|v| v.display == display && v.target.is_none()) {
                // SAFETY: target is valid.
                unsafe {
                    if let (Some(c), true) = (v.clear, full_clear(v)) {
                        c3d::C3D_RenderTargetClear(target, c3d::C3D_CLEAR_ALL, rgba_u32(c), 0);
                    }
                    c3d::C3D_FrameDrawOn(target);
                }
                if trace {
                    goat_core::info!("stereo frame: view on {:?} eye {eye}", display);
                }
                if !debug("nodraw") {
                    self.draw_view(target, v, eye, slider, &mut stats);
                }
            }
        }
        // SAFETY: ends the frame started above.
        unsafe { c3d::C3D_FrameEnd(0) };
        if trace {
            goat_core::info!("stereo frame: end ok");
        }
        stats
    }
}
