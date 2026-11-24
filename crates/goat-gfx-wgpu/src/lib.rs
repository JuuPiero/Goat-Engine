//! Desktop graphics backend on wgpu. On Windows / Linux it runs on **Vulkan** (forced unless
//! `GOAT_WGPU_BACKEND` says otherwise), Metal on Apple, WebGPU on the web.
//!
//! - One window surface; each engine [`Display`] is a rectangle of it ([`DisplayLayout`]): the
//!   whole window normally, or the 3DS's two screens stacked (preview of a 3DS game on PC).
//! - The standard material = `standard.wgsl` (per-pixel Lambert + Blinn-Phong, like the 3DS).
//! - Per frame: one uniform block per view (group 0) and per draw (group 1, dynamic offsets into
//!   one buffer), a bind group per material (group 2: texture + sampler).
//! - `request_screenshot` saves the next frame as PNG (tests, docs).

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use goat_core::handle::Pool;
use goat_core::math::{Mat4, Vec3};
use goat_core::Color;
use goat_gfx::*;

/// Where each display sits in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayLayout {
    /// One display covering the window.
    Window,
    /// Nintendo 3DS: top screen 400×240 above the bottom screen 320×240 (scaled to the window).
    Dual3ds,
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    bounds: Bounds,
}

/// Index data padded to 4 bytes (wgpu copy alignment).
fn index_bytes(mesh: &MeshData) -> Vec<u8> {
    let mut b: Vec<u8> = bytemuck::cast_slice(&mesh.indices).to_vec();
    b.resize(b.len().next_multiple_of(4).max(4), 0);
    b
}

fn vertex_bytes(mesh: &MeshData) -> Vec<u8> {
    let mut b = mesh.vertex_bytes().to_vec();
    b.resize(b.len().next_multiple_of(4).max(Vertex::SIZE), 0);
    b
}

/// Buffers with room to grow (`update_mesh` writes in place while the data fits).
fn upload_mesh(device: &wgpu::Device, queue: &wgpu::Queue, mesh: &MeshData) -> GpuMesh {
    let (vb, ib) = (vertex_bytes(mesh), index_bytes(mesh));
    let buffer = |label, size: usize, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size as u64, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let vertices = buffer("vertices", vb.len(), wgpu::BufferUsages::VERTEX);
    let indices = buffer("indices", ib.len(), wgpu::BufferUsages::INDEX);
    queue.write_buffer(&vertices, 0, &vb);
    queue.write_buffer(&indices, 0, &ib);
    GpuMesh { vertices, indices, index_count: mesh.indices.len() as u32, bounds: mesh.bounds() }
}

struct GpuTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
}

struct GpuMaterial {
    desc: MaterialDesc,
    bind_group: wgpu::BindGroup,
    texture: Option<TextureId>,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuLight {
    /// xyz = world position, w = range
    position_range: [f32; 4],
    /// xyz = direction the light travels, w = kind (0 directional, 1 point, 2 spot)
    direction_kind: [f32; 4],
    color: [f32; 4],
    /// x = cos inner, y = cos outer (spot)
    spot: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    view_proj: [[f32; 4]; 4],
    /// xyz = camera position, w = light count
    camera_pos: [f32; 4],
    /// xyz = camera forward
    camera_forward: [f32; 4],
    ambient: [f32; 4],
    /// Hemisphere ambient from below, w = 1 when on.
    ambient_ground: [f32; 4],
    /// rgb = fog color, w = 1 when fog is on
    fog_color: [f32; 4],
    /// x = start, y = end
    fog_range: [f32; 4],
    lights: [GpuLight; MAX_LIGHTS],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ObjectUniform {
    model: [[f32; 4]; 4],
    normal_matrix: [[f32; 4]; 4],
    base_color: [f32; 4],
    specular: [f32; 4],
    emissive: [f32; 4],
    flags: [f32; 4],
    /// x = toon steps, y = rim power, z = outline width
    toon: [f32; 4],
    rim: [f32; 4],
    outline: [f32; 4],
    /// x = kind + 1 (0 = none), y = scale, z = speed, w = time
    procedural: [f32; 4],
    proc_a: [f32; 4],
    proc_b: [f32; 4],
}

const UNIFORM_ALIGN: u64 = 256;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PipelineKey {
    blend: u8,
    cull: u8,
    depth_write: bool,
    depth_test: bool,
    /// The outline pass (back faces pushed out, flat color).
    outline: bool,
}

pub struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    adapter_name: String,
    layout: DisplayLayout,
    depth: wgpu::TextureView,
    shader: wgpu::ShaderModule,
    clear_shader: wgpu::ShaderModule,
    frame_layout: wgpu::BindGroupLayout,
    object_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    clear_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    clear_pipeline: wgpu::RenderPipeline,
    clear_depth_pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    uniform_capacity: u64,
    meshes: Pool<MeshData>,
    gpu_meshes: HashMap<u32, GpuMesh>,
    textures: Pool<TextureData>,
    gpu_textures: HashMap<u32, GpuTexture>,
    materials: Pool<MaterialDesc>,
    gpu_materials: HashMap<u32, GpuMaterial>,
    white: GpuTexture,
    /// Stand-in reflection (materials without an env map).
    black_cube: GpuTexture,
    /// Texture ids that are cube maps.
    cubes: std::collections::HashSet<u32>,
    screenshot: Option<std::path::PathBuf>,
}

impl WgpuBackend {
    /// Creates the backend on a window (`target` = e.g. an `Arc<winit::window::Window>`).
    /// Creates the backend on a window, blocking (desktop).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(target: impl Into<wgpu::SurfaceTarget<'static>>, width: u32, height: u32, layout: DisplayLayout) -> Result<Self, GfxError> {
        pollster::block_on(Self::new_async(target, width, height, layout))
    }

    /// Creates the backend (async: the web can't block; a canvas is a `SurfaceTarget` there).
    pub async fn new_async(target: impl Into<wgpu::SurfaceTarget<'static>>, width: u32, height: u32, layout: DisplayLayout) -> Result<Self, GfxError> {
        let backends = match std::env::var("GOAT_WGPU_BACKEND").as_deref() {
            Ok("dx12") => wgpu::Backends::DX12,
            Ok("metal") => wgpu::Backends::METAL,
            Ok("gl") => wgpu::Backends::GL,
            Ok("any") => wgpu::Backends::all(),
            _ if cfg!(target_arch = "wasm32") => wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
            _ if cfg!(any(target_os = "windows", target_os = "linux", target_os = "android")) => wgpu::Backends::VULKAN,
            _ => wgpu::Backends::PRIMARY,
        };
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends, ..Default::default() });
        let surface = instance.create_surface(target).map_err(|e| GfxError::Backend(format!("surface: {e}")))?;
        let adapter = (instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }).await)
        .map_err(|e| GfxError::Unsupported(format!("no GPU adapter for {backends:?}: {e}")))?;
        let info = adapter.get_info();
        let adapter_name = format!("wgpu/{:?} ({})", info.backend, info.name);
        let (device, queue) = (adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("goat"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }).await)
        .map_err(|e| GfxError::Backend(format!("device: {e}")))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        let usage = if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        };
        let config = wgpu::SurfaceConfiguration {
            usage,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        let depth = make_depth(&device, config.width, config.height);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("standard"), source: wgpu::ShaderSource::Wgsl(include_str!("standard.wgsl").into()) });
        let clear_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("clear"), source: wgpu::ShaderSource::Wgsl(include_str!("clear.wgsl").into()) });
        let uniform_entry = |vis| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: vis,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: None },
            count: None,
        };
        let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("frame"), entries: &[uniform_entry(vf)] });
        let object_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("object"), entries: &[uniform_entry(vf)] });
        let clear_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("clear"), entries: &[uniform_entry(wgpu::ShaderStages::FRAGMENT)] });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                // emissive map
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                // reflection cube map + its sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::Cube, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("standard"),
            bind_group_layouts: &[&frame_layout, &object_layout, &material_layout],
            push_constant_ranges: &[],
        });
        // clears: color + depth, or depth only (cameras that draw over earlier ones)
        let clear_pipeline_with = |write_mask| {
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("clear"), bind_group_layouts: &[&clear_layout], push_constant_ranges: &[] });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("clear"),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &clear_shader, entry_point: Some("vs_clear"), compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState {
                    module: &clear_shader,
                    entry_point: Some("fs_clear"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Always,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let clear_pipeline = clear_pipeline_with(wgpu::ColorWrites::ALL);
        let clear_depth_pipeline = clear_pipeline_with(wgpu::ColorWrites::empty());
        let uniform_capacity = 256 * UNIFORM_ALIGN;
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: uniform_capacity,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let white = upload_texture(&device, &queue, &TextureData::solid(Color::WHITE));
        let black_cube = upload_cubemap(&device, &queue, &std::array::from_fn(|_| TextureData::solid(Color::BLACK)));
        Ok(WgpuBackend {
            device,
            queue,
            surface,
            config,
            adapter_name,
            layout,
            depth,
            shader,
            clear_shader,
            frame_layout,
            object_layout,
            material_layout,
            clear_layout,
            pipeline_layout,
            pipelines: HashMap::new(),
            clear_pipeline,
            clear_depth_pipeline,
            uniforms,
            uniform_capacity,
            meshes: Pool::new(),
            gpu_meshes: HashMap::new(),
            textures: Pool::new(),
            gpu_textures: HashMap::new(),
            materials: Pool::new(),
            gpu_materials: HashMap::new(),
            white,
            black_cube,
            cubes: Default::default(),
            screenshot: None,
        })
    }

    /// The window was resized.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = make_depth(&self.device, width, height);
    }

    /// Saves the next rendered frame as a PNG.
    pub fn request_screenshot(&mut self, path: impl Into<std::path::PathBuf>) {
        self.screenshot = Some(path.into());
    }

    /// Switches the display layout (the editor: Game view as one window or the two 3DS screens).
    pub fn set_layout(&mut self, layout: DisplayLayout) {
        self.layout = layout;
    }

    pub fn layout(&self) -> DisplayLayout {
        self.layout
    }

    /// Window-pixel rectangle (x, y, w, h) of a display.
    pub fn display_rect(&self, d: Display) -> Option<(f32, f32, f32, f32)> {
        let (w, h) = (self.config.width as f32, self.config.height as f32);
        match (self.layout, d) {
            (DisplayLayout::Window, Display::Main) => Some((0.0, 0.0, w, h)),
            (DisplayLayout::Window, Display::Secondary) => None,
            (DisplayLayout::Dual3ds, _) => {
                // 400×480 logical, scaled uniformly and centered
                let s = (w / 400.0).min(h / 480.0);
                let ox = (w - 400.0 * s) * 0.5;
                let oy = (h - 480.0 * s) * 0.5;
                Some(match d {
                    Display::Main => (ox, oy, 400.0 * s, 240.0 * s),
                    Display::Secondary => (ox + 40.0 * s, oy + 240.0 * s, 320.0 * s, 240.0 * s),
                })
            }
        }
    }

    fn pipeline(&mut self, m: &MaterialDesc) {
        self.pipeline_with(m, false);
        if m.outline_width > 0.0 {
            self.pipeline_with(m, true);
        }
    }

    fn pipeline_with(&mut self, m: &MaterialDesc, outline: bool) {
        let key = PipelineKey { blend: m.blend as u8, cull: m.cull as u8, depth_write: m.depth_write, depth_test: m.depth_test, outline };
        let (device, layout, shader, format) = (&self.device, &self.pipeline_layout, &self.shader, self.config.format);
        let _ = self.pipelines.entry(key).or_insert_with(|| {
            let blend = match m.blend {
                _ if outline => None,
                BlendMode::Opaque => None,
                BlendMode::Alpha => Some(wgpu::BlendState::ALPHA_BLENDING),
                BlendMode::Additive => Some(wgpu::BlendState {
                    color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                    alpha: wgpu::BlendComponent::OVER,
                }),
            };
            let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("standard"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some(if outline { "vs_outline" } else { "vs_main" }),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout { array_stride: Vertex::SIZE as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs }],
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(if outline { "fs_outline" } else { "fs_main" }),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: match m.cull {
                        _ if outline => Some(wgpu::Face::Front),
                        CullMode::None => None,
                        CullMode::Back => Some(wgpu::Face::Back),
                        CullMode::Front => Some(wgpu::Face::Front),
                    },
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: m.depth_write || outline,
                    depth_compare: if m.depth_test { wgpu::CompareFunction::LessEqual } else { wgpu::CompareFunction::Always },
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        });
    }

    fn material_bind_group(&self, m: &MaterialDesc) -> wgpu::BindGroup {
        let get = |id: Option<TextureId>, cube: bool| id.filter(|t| self.cubes.contains(&t.index()) == cube).and_then(|t| self.gpu_textures.get(&t.index()));
        let t = get(m.texture, false).unwrap_or(&self.white);
        let e = get(m.emissive_texture, false).unwrap_or(&self.white);
        let c = get(m.env_map, true).unwrap_or(&self.black_cube);
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material"),
            layout: &self.material_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&t.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&t.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&e.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&c.view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&c.sampler) },
            ],
        })
    }

    #[cfg(feature = "screenshot")]
    fn save_screenshot(&mut self, texture: &wgpu::Texture, path: &std::path::Path) {
        let (w, h) = (self.config.width, self.config.height);
        let row = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("screenshot"), size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::Wait);
        let data = slice.get_mapped_range();
        let bgra = matches!(self.config.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let line = &data[(y * row) as usize..(y * row + w * 4) as usize];
            for px in line.chunks_exact(4) {
                if bgra {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                } else {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
        }
        drop(data);
        buf.unmap();
        match std::fs::File::create(path) {
            Ok(f) => {
                let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                match enc.write_header().and_then(|mut wr| wr.write_image_data(&rgba)) {
                    Ok(()) => goat_core::info!("screenshot saved: {}", path.display()),
                    Err(e) => goat_core::error!("screenshot: {e}"),
                }
            }
            Err(e) => goat_core::error!("screenshot {}: {e}", path.display()),
        }
    }
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

fn make_depth(device: &wgpu::Device, w: u32, h: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn upload_texture(device: &wgpu::Device, queue: &wgpu::Queue, t: &TextureData) -> GpuTexture {
    let size = wgpu::Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 };
    let levels = t.mip_chain(1);
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("texture"),
        size,
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (w, h, rgba)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(*h) },
            wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
        );
    }
    let filter = if t.filter == Filter::Linear { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest };
    let wrap = if t.wrap == Wrap::Repeat { wgpu::AddressMode::Repeat } else { wgpu::AddressMode::ClampToEdge };
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wrap,
        address_mode_v: wrap,
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: if levels.len() > 1 { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest },
        ..Default::default()
    });
    GpuTexture { view: tex.create_view(&Default::default()), sampler, texture: tex }
}

/// A cube map from 6 faces (+X, -X, +Y, -Y, +Z, -Z), resized to the first face's size.
fn upload_cubemap(device: &wgpu::Device, queue: &wgpu::Queue, faces: &[TextureData; 6]) -> GpuTexture {
    let n = faces[0].width.max(1);
    let size = wgpu::Extent3d { width: n, height: n, depth_or_array_layers: 6 };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cubemap"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (i, f) in faces.iter().enumerate() {
        // faces must match: nearest resample to n × n
        let px: Vec<u8> = if f.width == n && f.height == n {
            f.rgba.clone()
        } else {
            (0..n * n).flat_map(|k| {
                let (x, y) = (k % n * f.width / n, k / n * f.height / n);
                let o = ((y * f.width + x) * 4) as usize;
                f.rgba.get(o..o + 4).map(|s| [s[0], s[1], s[2], s[3]]).unwrap_or([0, 0, 0, 255])
            })
            .collect()
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: i as u32 }, aspect: wgpu::TextureAspect::All },
            &px,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(n * 4), rows_per_image: Some(n) },
            wgpu::Extent3d { width: n, height: n, depth_or_array_layers: 1 },
        );
    }
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
    let view = tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::Cube), ..Default::default() });
    GpuTexture { view, sampler, texture: tex }
}

fn projection_matrix(p: Projection, aspect: f32) -> Mat4 {
    match p {
        Projection::Perspective { fov_y_radians, near, far } => Mat4::perspective_rh(fov_y_radians, aspect, near, far),
        Projection::Orthographic { height, near, far } => {
            let h = height * 0.5;
            let w = h * aspect;
            Mat4::orthographic_rh(-w, w, -h, h, near, far)
        }
    }
}

/// Material / light / clear colors are sRGB-authored: an sRGB target blends in linear space.
fn c4(c: Color, linear: bool) -> [f32; 4] {
    if linear { c.to_linear().to_array() } else { c.to_array() }
}

impl GraphicsBackend for WgpuBackend {
    fn name(&self) -> &str {
        &self.adapter_name
    }

    fn displays(&self) -> Vec<DisplayInfo> {
        match self.layout {
            DisplayLayout::Window => vec![DisplayInfo { display: Display::Main, width: self.config.width, height: self.config.height, stereo: false }],
            DisplayLayout::Dual3ds => vec![
                DisplayInfo { display: Display::Main, width: 400, height: 240, stereo: false },
                DisplayInfo { display: Display::Secondary, width: 320, height: 240, stereo: false },
            ],
        }
    }

    fn create_mesh(&mut self, mesh: &MeshData) -> Result<MeshId, GfxError> {
        let gpu = upload_mesh(&self.device, &self.queue, mesh);
        let id = self.meshes.insert(MeshData::default());
        self.gpu_meshes.insert(id.index(), gpu);
        Ok(id)
    }

    fn update_texture(&mut self, id: TextureId, tex: &TextureData) -> Result<(), GfxError> {
        let meta = self.textures.get(id).ok_or_else(|| GfxError::Backend("update_texture: unknown texture".into()))?;
        if (meta.width, meta.height) != (tex.width, tex.height) || tex.rgba.len() != (tex.width * tex.height * 4) as usize {
            return Err(GfxError::Unsupported("update_texture: size changed".into()));
        }
        let g = self.gpu_textures.get(&id.index()).ok_or_else(|| GfxError::Backend("update_texture: no GPU texture".into()))?;
        self.queue.write_texture(
            g.texture.as_image_copy(),
            &tex.rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(tex.width * 4), rows_per_image: Some(tex.height) },
            wgpu::Extent3d { width: tex.width, height: tex.height, depth_or_array_layers: 1 },
        );
        Ok(())
    }

    fn update_mesh(&mut self, id: MeshId, mesh: &MeshData) -> Result<(), GfxError> {
        if self.meshes.get(id).is_none() {
            return Err(GfxError::Backend("update_mesh: unknown mesh".into()));
        }
        let (vb, ib) = (vertex_bytes(mesh), index_bytes(mesh));
        let fits = self.gpu_meshes.get(&id.index()).is_some_and(|g| g.vertices.size() >= vb.len() as u64 && g.indices.size() >= ib.len() as u64);
        if fits {
            let g = self.gpu_meshes.get_mut(&id.index()).unwrap();
            // queue writes are staged: frames already submitted keep their data
            self.queue.write_buffer(&g.vertices, 0, &vb);
            self.queue.write_buffer(&g.indices, 0, &ib);
            g.index_count = mesh.indices.len() as u32;
            g.bounds = mesh.bounds();
        } else {
            // grow with headroom (per-frame geometry usually grows a little at a time)
            let mut grown = mesh.clone();
            let extra_v = grown.vertices.len().next_power_of_two() - grown.vertices.len();
            grown.vertices.extend(std::iter::repeat_n(Vertex::default(), extra_v));
            let extra_i = grown.indices.len().next_power_of_two() - grown.indices.len();
            grown.indices.extend(std::iter::repeat_n(0, extra_i));
            let mut gpu = upload_mesh(&self.device, &self.queue, &grown);
            gpu.index_count = mesh.indices.len() as u32;
            gpu.bounds = mesh.bounds();
            self.gpu_meshes.insert(id.index(), gpu);
        }
        Ok(())
    }

    fn create_texture(&mut self, tex: &TextureData) -> Result<TextureId, GfxError> {
        let gpu = upload_texture(&self.device, &self.queue, tex);
        let id = self.textures.insert(tex.clone_meta());
        self.gpu_textures.insert(id.index(), gpu);
        Ok(id)
    }

    fn create_cubemap(&mut self, faces: &[TextureData; 6]) -> Result<TextureId, GfxError> {
        let gpu = upload_cubemap(&self.device, &self.queue, faces);
        let id = self.textures.insert(faces[0].clone_meta());
        self.gpu_textures.insert(id.index(), gpu);
        self.cubes.insert(id.index());
        Ok(id)
    }

    fn create_material(&mut self, mat: &MaterialDesc) -> Result<MaterialId, GfxError> {
        let bind_group = self.material_bind_group(mat);
        let id = self.materials.insert(mat.clone());
        self.gpu_materials.insert(id.index(), GpuMaterial { desc: mat.clone(), bind_group, texture: mat.texture });
        Ok(id)
    }

    fn material_desc(&self, id: MaterialId) -> Option<MaterialDesc> {
        self.materials.get(id).cloned()
    }

    fn update_material(&mut self, id: MaterialId, mat: &MaterialDesc) {
        if self.materials.get(id).is_none() {
            return;
        }
        let rebind = self.gpu_materials.get(&id.index()).map(|g| g.texture != mat.texture || g.desc.emissive_texture != mat.emissive_texture || g.desc.env_map != mat.env_map).unwrap_or(true);
        let bind_group = if rebind { Some(self.material_bind_group(mat)) } else { None };
        if let Some(g) = self.gpu_materials.get_mut(&id.index()) {
            g.desc = mat.clone();
            g.texture = mat.texture;
            if let Some(b) = bind_group {
                g.bind_group = b;
            }
        }
    }

    fn mesh_bounds(&self, id: MeshId) -> Option<Bounds> {
        self.meshes.get(id)?;
        self.gpu_meshes.get(&id.index()).map(|m| m.bounds)
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
        if self.materials.remove(id).is_some() {
            self.gpu_materials.remove(&id.index());
        }
    }

    fn render(&mut self, views: &[RenderView]) -> FrameStats {
        let mut stats = FrameStats::default();
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return stats;
            }
            Err(e) => {
                goat_core::warn!("surface: {e}");
                return stats;
            }
        };
        let target = frame.texture.create_view(&Default::default());

        // uniforms: [clear colors and frames per view][objects]
        let draws: usize = views.iter().map(|v| v.items.len()).sum();
        let frame_slots = (std::mem::size_of::<FrameUniform>() as u64).div_ceil(UNIFORM_ALIGN);
        let object_slots = (std::mem::size_of::<ObjectUniform>() as u64).div_ceil(UNIFORM_ALIGN);
        let needed = ((views.len() as u64) * (1 + frame_slots) + draws as u64 * object_slots + 1) * UNIFORM_ALIGN;
        if needed > self.uniform_capacity {
            self.uniform_capacity = needed.next_power_of_two();
            self.uniforms = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("uniforms"),
                size: self.uniform_capacity,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let mut data = vec![0u8; needed as usize];
        let mut slot = 0u64;
        let mut put = |bytes: &[u8]| -> u32 {
            let off = slot * UNIFORM_ALIGN;
            data[off as usize..off as usize + bytes.len()].copy_from_slice(bytes);
            slot += (bytes.len() as u64).div_ceil(UNIFORM_ALIGN).max(1);
            off as u32
        };
        struct Planned {
            rect: (f32, f32, f32, f32),
            clear_off: Option<u32>,
            clear_depth: bool,
            frame_off: u32,
            objects: Vec<(MeshId, MaterialId, u32)>,
        }
        let lin = self.config.format.is_srgb();
        let mut planned = Vec::with_capacity(views.len());
        for v in views {
            let Some((dx, dy, dw, dh)) = self.display_rect(v.display) else { continue };
            let rect = (dx + v.viewport.x * dw, dy + v.viewport.y * dh, (v.viewport.width * dw).max(1.0), (v.viewport.height * dh).max(1.0));
            let clear_off = v.clear.map(|c| put(bytemuck::bytes_of(&c4(c, lin))));
            let proj = projection_matrix(v.projection, rect.2 / rect.3);
            let cam_world = v.view.inverse();
            let cam = cam_world.w_axis.truncate();
            let mut lights = [GpuLight::zeroed(); MAX_LIGHTS];
            for (g, l) in lights.iter_mut().zip(v.lighting.lights.iter().take(MAX_LIGHTS)) {
                let (kind, spot) = match l.kind {
                    LightKind::Directional => (0.0, [0.0; 4]),
                    LightKind::Point => (1.0, [0.0; 4]),
                    LightKind::Spot { cos_inner, cos_outer } => (2.0, [cos_inner, cos_outer, 0.0, 0.0]),
                };
                *g = GpuLight {
                    position_range: l.position.extend(l.range).into(),
                    direction_kind: l.direction.normalize_or(Vec3::NEG_Y).extend(kind).into(),
                    color: c4(l.color, lin),
                    spot,
                };
            }
            let fog = v.fog;
            let fu = FrameUniform {
                view_proj: (proj * v.view).to_cols_array_2d(),
                camera_pos: cam.extend(v.lighting.lights.len().min(MAX_LIGHTS) as f32).into(),
                // w = 1: vertex colors are sRGB-authored and must be linearized (sRGB target)
                camera_forward: cam_world.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Z).extend(if lin { 1.0 } else { 0.0 }).into(),
                ambient: c4(v.lighting.ambient, lin),
                ambient_ground: v.lighting.ambient_ground.map(|g| { let c = c4(g, lin); [c[0], c[1], c[2], 1.0] }).unwrap_or([0.0; 4]),
                fog_color: fog.map(|f| { let c = c4(f.color, lin); [c[0], c[1], c[2], 1.0] }).unwrap_or([0.0; 4]),
                fog_range: fog.map(|f| [f.start, f.end.max(f.start + 1e-3), 0.0, 0.0]).unwrap_or([0.0; 4]),
                lights,
            };
            let frame_off = put(bytemuck::bytes_of(&fu));
            let order = v.draw_order(|m| self.gpu_materials.get(&m.index()).map(|m| m.desc.blend));
            let mut objects = Vec::with_capacity(v.items.len());
            for it in order.iter().map(|&i| &v.items[i]) {
                let Some(m) = self.gpu_materials.get(&it.material.index()) else { continue };
                let d = &m.desc;
                let ou = ObjectUniform {
                    model: it.model.to_cols_array_2d(),
                    normal_matrix: it.model.inverse().transpose().to_cols_array_2d(),
                    base_color: c4(d.base_color, lin),
                    specular: { let s = c4(d.specular, lin); [s[0], s[1], s[2], d.shininess.max(1.0)] },
                    emissive: c4(d.emissive, lin),
                    toon: [d.toon_steps as f32, d.rim_power.max(0.01), d.outline_width, 0.0],
                    rim: c4(d.rim_color, lin),
                    outline: c4(d.outline_color, lin),
                    procedural: d.procedural.map(|p| [p.kind as u8 as f32 + 1.0, p.scale, p.speed, v.time]).unwrap_or([0.0; 4]),
                    proc_a: d.procedural.map(|p| c4(p.color_a, lin)).unwrap_or([0.0; 4]),
                    proc_b: d.procedural.map(|p| c4(p.color_b, lin)).unwrap_or([0.0; 4]),
                    flags: [if d.lighting == LightingModel::Lit { 1.0 } else { 0.0 }, if d.fog { 1.0 } else { 0.0 }, if d.env_map.is_some() { d.reflectivity } else { 0.0 }, if d.alpha_dither { -d.alpha_cutoff } else { d.alpha_cutoff }],
                };
                objects.push((it.mesh, it.material, put(bytemuck::bytes_of(&ou))));
            }
            planned.push(Planned { rect, clear_off, clear_depth: v.clear_depth, frame_off, objects });
        }
        self.queue.write_buffer(&self.uniforms, 0, &data);

        // pipelines for every material drawn
        let mats: Vec<MaterialDesc> = planned.iter().flat_map(|p| p.objects.iter()).filter_map(|(_, m, _)| self.gpu_materials.get(&m.index()).map(|g| g.desc.clone())).collect();
        for m in &mats {
            self.pipeline(m);
        }

        let frame_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &self.frame_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &self.uniforms, offset: 0, size: wgpu::BufferSize::new(std::mem::size_of::<FrameUniform>() as u64) }) }],
        });
        let object_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("object"),
            layout: &self.object_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &self.uniforms, offset: 0, size: wgpu::BufferSize::new(std::mem::size_of::<ObjectUniform>() as u64) }) }],
        });
        let clear_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("clear"),
            layout: &self.clear_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &self.uniforms, offset: 0, size: wgpu::BufferSize::new(16) }) }],
        });

        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.02, g: 0.02, b: 0.025, a: 1.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            for p in &planned {
                let (x, y, w, h) = p.rect;
                pass.set_viewport(x, y, w, h, 0.0, 1.0);
                let (sx, sy) = (x.max(0.0) as u32, y.max(0.0) as u32);
                let sw = (w as u32).min(self.config.width.saturating_sub(sx));
                let sh = (h as u32).min(self.config.height.saturating_sub(sy));
                if sw == 0 || sh == 0 {
                    continue;
                }
                pass.set_scissor_rect(sx, sy, sw, sh);
                if let Some(off) = p.clear_off {
                    pass.set_pipeline(&self.clear_pipeline);
                    pass.set_bind_group(0, &clear_bg, &[off]);
                    pass.draw(0..3, 0..1);
                } else if p.clear_depth {
                    pass.set_pipeline(&self.clear_depth_pipeline);
                    pass.set_bind_group(0, &clear_bg, &[0]);
                    pass.draw(0..3, 0..1);
                }
                pass.set_bind_group(0, &frame_bg, &[p.frame_off]);
                for &(mesh, material, off) in &p.objects {
                    let (Some(gm), Some(mat)) = (self.gpu_meshes.get(&mesh.index()), self.gpu_materials.get(&material.index())) else { continue };
                    if gm.index_count == 0 {
                        continue;
                    }
                    let key = PipelineKey { blend: mat.desc.blend as u8, cull: mat.desc.cull as u8, depth_write: mat.desc.depth_write, depth_test: mat.desc.depth_test, outline: false };
                    let Some(pipe) = self.pipelines.get(&key) else { continue };
                    pass.set_pipeline(pipe);
                    pass.set_bind_group(1, &object_bg, &[off]);
                    pass.set_bind_group(2, &mat.bind_group, &[]);
                    pass.set_vertex_buffer(0, gm.vertices.slice(..));
                    pass.set_index_buffer(gm.indices.slice(..), wgpu::IndexFormat::Uint16);
                    pass.draw_indexed(0..gm.index_count, 0, 0..1);
                    stats.draw_calls += 1;
                    stats.triangles += gm.index_count / 3;
                    if mat.desc.outline_width > 0.0 {
                        if let Some(p) = self.pipelines.get(&PipelineKey { outline: true, ..key }) {
                            pass.set_pipeline(p);
                            pass.draw_indexed(0..gm.index_count, 0, 0..1);
                            stats.draw_calls += 1;
                        }
                    }
                }
            }
        }
        self.queue.submit([enc.finish()]);
        if let Some(path) = self.screenshot.take() {
            if cfg!(not(feature = "screenshot")) {
                goat_core::warn!("screenshot {}: goat-gfx-wgpu was built without the `screenshot` feature", path.display());
            } else if self.config.usage.contains(wgpu::TextureUsages::COPY_SRC) {
                #[cfg(feature = "screenshot")]
                self.save_screenshot(&frame.texture, &path);
            } else {
                goat_core::warn!("screenshot: the surface can't be copied on this GPU");
            }
        }
        frame.present();
        let _ = (&self.clear_shader,);
        stats
    }
}
