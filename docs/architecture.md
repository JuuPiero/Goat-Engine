# GoatEngine architecture

## Layers and dependency rule

```
                 games/*  (depend on goat-runtime only)
                    │
goat-core ◄── goat-gfx ◄── goat-runtime
                 ▲                ▲
   goat-gfx-wgpu │   goat-gfx-citro3d      (implement goat_gfx::GraphicsBackend)
                 │                ▲
   goat-platform-desktop   goat-platform-3ds  (main loop, input → goat_runtime::Input)
                 ▲                ▲
   players/desktop         players/3ds        (binary: platform::run(game))
tools/goat: Builder per platform (check / build / run), no engine dependency
```

- A crate never depends on a crate to its right or below it in this diagram.
- Platform-specific code (FFI, OS APIs) lives only in backends and platforms.
- Adding a platform must not change `goat-core`, `goat-gfx`, `goat-runtime` or games.

## Modules: a build contains only what the game uses
- **Subsystems are separate crates** (physics, audio, UI, animation, particles, 2D, netcode... —
  `goat-physics`, `goat-audio`, ...). A game depends on the ones it uses; a crate that isn't a
  dependency is never compiled into the build.
- **Optional parts of a crate are cargo features**, off unless a player turns them on. Example:
  `goat-gfx-wgpu/screenshot` (the PNG encoder) is enabled by the sample's desktop player, never by
  shipped games.
- Heavy third-party dependencies go behind a feature or into their own crate. Never put them in
  `goat-core`, `goat-gfx` or `goat-runtime`.
- Release profile: `lto = "thin"`, `opt-level = 3`. Rust drops unreferenced functions at link time,
  so code a game never calls costs nothing even inside a linked crate.
- The core keeps only what every game needs: transforms, cameras, mesh rendering, lights, input,
  behaviours.

## goat-core
- `math`: re-export of glam (`Vec2/3/4`, `Quat`, `Mat4`, `Affine3A`, `EulerRot`). Right-handed, Y up, camera looks down −Z.
- `Color`: f32 RGBA, **sRGB-authored**. `to_linear()` is used by linear-space renderers. `to_rgba8()`.
- `Handle<T>` / `Pool<T>`: generational indices. Backends key their GPU objects by `handle.index()` and keep the CPU description in a `Pool`.
- `log`: `set_sink(fn)` plus the `info!` / `warn!` / `error!` / `debug!` macros. The platform installs the sink (stderr on desktop, `svcOutputDebugString` on 3DS).
- `Time`: elapsed, delta (clamped by `max_delta`) and frame.

## goat-gfx: backend-neutral rendering API
- `GraphicsBackend`:
  - `create_mesh` / `create_texture` / `create_material`, `update_material`, `destroy_*`
  - `displays()`
  - `render(&[RenderView]) -> FrameStats`
  - Resources are immutable descriptions. A backend may convert them to its own format: tiled textures on PICA, GPU buffers on wgpu.
- `Vertex` = position, normal, uv, color (f32, 48 bytes, `#[repr(C)]`). Indices are `u16`, which is PICA-friendly and caps a mesh at 65 536 vertices.
- `MaterialDesc`:
  - base_color, texture, lighting (Unlit / Lit), specular + shininess, emissive
  - blend (Opaque / Alpha / Additive), cull, depth_write
- `RenderView` is everything one camera draws:
  - display, normalized viewport, view matrix, projection (Perspective / Orthographic by height), clear color
  - lighting: ambient + up to `MAX_LIGHTS` (8) `Light`s (directional / point / spot, world space, color × intensity; falloff = `Light::attenuation`, cone = `Light::spot_factor`); `fog: Option<Fog>` (linear, view depth)
  - items: mesh, material, model matrix
  - depth: the camera order on a display
- `RenderView::draw_order(blend_of)` is the shared sort: opaque front to back, then blended back to front by the view-space z of each object's origin.
- Displays: `Main` = 3DS top screen (400×240, stereo capable) or the desktop window. `Secondary` = 3DS touch screen (320×240). A backend reports which displays it has. The desktop's `Dual3ds` layout has both.

## goat-runtime: Engine and ECS
- The ECS is hecs. Components:
  - `Name`, `Transform` (local position / rotation / scale), `GlobalTransform` (written by the engine), `Parent(Entity)`
  - `Camera` (display, viewport, projection, clear, depth), `MeshRenderer` (mesh, material, visible), `DirectionalLight`, `PointLight`, `SpotLight` (Unity-style angles in degrees), `RenderSettings` (ambient + fog, first one found)
  - `Scripts`
- `Game` trait: `start(ctx)`, `update(ctx)`, `title()`.
- `Behaviour` trait (`start` / `update` with `(ctx, me)`) is the single interface for every scripting language:
  - Rust implements it directly.
  - Language hosts (C ABI, QuickJS, CoreCLR) will be behaviours that forward into their VM.
- `Engine::frame(dt, gfx)` runs the steps below in order:
  1. Time
  2. `Game::start` once, then `Game::update`
  3. behaviours
  4. `propagate_transforms` (parent chains)
  5. `extract_views`: per enabled camera, frustum culling (mesh bounding spheres from `GraphicsBackend::mesh_bounds`, scaled by the largest axis) and light picking (directional first, then point / spot lights reaching the frustum, nearest first, 8 max)
  6. `gfx.render`
- While a behaviour runs, its `Scripts` component is taken off the entity, so the behaviour can borrow the world mutably. As a result, ECS iteration order changes from frame to frame.
- `Context` holds the world, time, input, `&mut dyn GraphicsBackend`, displays, plus helpers (`find(name)`, `transform_mut(e)`, `quit()`).
- `Input` is portable: 3DS-style `Button`s (A/B/X/Y/L/R/ZL/ZR/Start/Select/D-pad), `Axis` (left / right stick), and an optional `pointer` on the Secondary display (touch, or the mouse on desktop). Platforms call `begin_frame`, `set` and `set_axis`.

## Plugins (subsystems)
- `goat_runtime::Plugin` (`build`, `fixed_update`, `update`, `late_update`): the only way a
  subsystem crate hooks into the frame. `Game::plugins()` lists them; the Engine takes the list out
  while calling them, so a plugin gets a normal `Context` (world, time, input, gfx).
- Fixed timestep: `Time::fixed_delta` (1/60), accumulator, at most 4 steps a frame; inside fixed
  steps `time.delta` = the step and `time.in_fixed_step` = true.
- Render data generated per frame (sprites, later text / particles / UI) goes through
  `GraphicsBackend::update_mesh` (wgpu: in-place queue writes, grows by powers of two; citro3d:
  double-buffered linear memory, because the GPU may still read last frame's buffers while the
  CPU builds the next) and helper entities with a `MeshRenderer`.
- `DrawItem::sorting_order` orders blended items before depth. The sort is stable, so equal keys keep
  their submission order. Sprite batches live in their own archetype (`SpriteBatch` marker), which
  keeps them in spawn order.

## 2D (goat-sprites)
- `Sprite` = texture + pixel rect + pivot (0..1 from the bottom-left) + pixels per unit;
  `SpriteRenderer` = sprite + tint + flips + layer + sorting order + blend.
- `SpritePlugin::late_update`:
  - collects the visible sprites as world-space quads;
  - sorts them by (sorting order, z, texture);
  - cuts the list into runs of equal (order, layer, texture, blend), up to 16 384 quads each;
  - sends each run as one mesh (`update_mesh`). Materials are cached per (texture, blend): Unlit,
    blended, no culling, no depth write, no fog.

## Assets (goat-assets)
- `AssetIo` reads files under a root:
  - 3DS: `romfs:/` (the platform mounts RomFS);
  - desktop: `GOAT_ASSETS`, then `<exe>/assets`, then `AssetsPlugin::dev_root` (the game's source folder).
- Keys:
  - a path relative to the root, always with `/`;
  - a sub-asset after `#`: `#mesh:<mesh>:<primitive>`, `#material:<i>`, `#texture:<material>`;
  - built-ins: `builtin:cube|sphere|plane|default`.
- `AssetServer` (a resource):
  - `texture` / `material` / `mesh` / `model` load and cache by key;
  - `key_of_*` maps GPU handles back to keys (scene saving);
  - `register_*` gives keys to resources made in code;
  - `spawn_model(_into)` builds the node hierarchy and tags the root with `ModelInstance`.
- PNG goes through the `png` crate (normalized to 8-bit, any color type → RGBA8). Import settings come from `<file>.import.json`. If `create_texture` returns `Unsupported` (the 3DS needs powers of two), the image is resized and uploaded again.
- glTF is read with the `gltf` crate without its `import` feature, so the `image` crate isn't pulled in:
  - buffers: GLB chunk, data URIs, files;
  - images: PNG only;
  - colors are linear in glTF and converted to sRGB (`Color::from_linear`);
  - roughness r → shininess 2/r⁴ − 2;
  - meshes are split to fit u16 indices;
  - the CPU vertex data is dropped after upload.

## Scenes and prefabs (goat-scene)
- File format: `SceneFile { version, entities: [EntityEntry { id, name, parent, model, prefab, components: {Name: json}, scripts: [json] }] }`.
  - `id`s are file-local.
  - `SCENE_VERSION` = 1; newer files are refused.
- `SceneRegistry` (a resource):
  - `ComponentCodec { name, save, load }` as plain fn pointers. Built-ins are in `builtins.rs`; subsystems add theirs (goat-sprites with feature `scene`).
  - Behaviours: `registry.behaviour::<B>(name)` for `B: Behaviour + Serialize + DeserializeOwned`. Lookup by `TypeId` through `Behaviour: AsAny`.
- Loading (`spawn_file`):
  1. Spawn every entry and map ids to entities.
  2. Run codecs in registry order; unknown components or behaviours log a warning and are skipped.
  3. Default Transform, Parent from the ids, roots under the given parent.
  4. Scripts, `model` (`spawn_model_into`), `prefab` (recursive, depth ≤ 16, tagged `PrefabInstance`).
- Saving (`save_world`):
  - covers every entity with a Transform, except `DontSave` ones and descendants of `ModelInstance` / `PrefabInstance` entities;
  - entities are sorted by entity id (spawn order), so load → save is stable (tested);
  - MeshRenderer / SpriteRenderer resources without an asset key are skipped with a warning.
- `load_scene(ctx, key, Single|Additive)`, `instantiate(ctx, key, parent)` and `save_scene(ctx)` take the registry out of the resources while they run.
- glam is built with `serde`: `Vec3` = `[x, y, z]`, `Quat` = `[x, y, z, w]` in behaviour fields.

## Text (goat-text)
- Fonts are TTF / OTF assets rasterized by fontdue at `RASTER_PX` (40) into one 512×512 RGBA atlas per font (white + coverage alpha), with a shelf packer. Glyphs are added on first use; when the atlas is full it is cleared and refilled the next frame.
- The atlas texture goes through `GraphicsBackend::update_texture`: same size, level 0 (wgpu `write_texture`; citro3d re-tiles and `C3D_TexLoadImage`).
- Layout (`layout`):
  - wraps at spaces against `max_width`, applies kerning and `line_spacing` × the font's line height;
  - aligns left / center / right, and vertically top / middle / bottom relative to the transform;
  - scales by `size / RASTER_PX`.
- Quads go to a `goat_sprites::Batcher` (shared with sprites: `Quad` + `Batcher::submit` are public for any quad producer).

## Physics (goat-physics)
- Rapier 0.22. A `Physics` resource owns the Rapier sets and pipeline, plus `tracked: Entity → body handle + last pose / velocities`.
- `fixed_update` → `Physics::step`:
  1. Remove bodies of despawned entities or removed colliders.
  2. Add entities with a `Collider`. The pose comes from the Transform chain (`world_matrix`), because GlobalTransform may be stale. Collider sizes are × the world scale. `mass` → density over the total volume.
  3. Apply script changes: a moved dynamic body is teleported, changed velocities are set, kinematic bodies take their next pose from the Transform.
  4. Step, with collision events through an `EventHandler` collecting into a mutex.
  5. Write dynamic poses back to Transform (made local under a Parent) and GlobalTransform, and velocities back to `RigidBody`.
  6. Events become `CollisionEvent { kind, a, b }`; `Physics::events` is cleared in `update`, after the behaviours of the frame have read it.
- `Collider.user_data` = `Entity::to_bits()`. `raycast` uses the `QueryPipeline` (updated by the step) and ignores triggers.

## Audio (goat-audio)
- `AudioClip`: interleaved i16, any channel count / rate. WAV parsing is our own (PCM 8/16/24/32, float32, EXTENSIBLE); Ogg goes through lewton.
- `Mixer`:
  - up to 32 voices (the oldest one-shot is dropped beyond that);
  - linear resampling by source rate / output rate × pitch, equal-power pan, f32 accumulation clamped to i16.
- `Audio` resource: `Arc<Mutex<Mixer>>`, a clip cache (assets or `add_clip`), listener pose. `spatialize` gives gain = min / max(d, min), 0 beyond max, and pan = dot(dir, listener right) × 0.8.
- `AudioPlugin::late_update` runs the sources (play on start, play / stop requests, spatial updates) and calls `Output::pump`.
- Outputs:
  - desktop: `cpal` default device, which mixes on its own thread;
  - 3DS: ndsp channel 0, stereo PCM16 at 32 728 Hz, 3 × 1024-frame wave buffers in linear memory, refilled when `NDSP_WBUF_DONE`;
  - `GOAT_AUDIO_DUMP`: mixes on the main thread into a WAV file, rewritten every second (verification).

## Animation (goat-animation)
- `Animator.states`: (clip, time, speed, weight, looping, fade rate).
- `AnimationPlugin::update` (before transform propagation):
  - samples each state's channels: binary search on the key times; step or linear; rotations nlerp'd in the same hemisphere;
  - accumulates per node and path weighted by state weight, then writes `Transform`s of the `ModelNodes` entities; total weight < 1 blends with the current pose.
- Skinned nodes get a Unity-like `SkinnedMeshRenderer` (goat-runtime components.rs) instead of MeshFilter + MeshRenderer: `shared` (submesh meshes, bind pose), `materials`, `bones` (joint entities), `root_bone` (top joint), `update_when_offscreen`, `quality` (Auto / 1 / 2 / 4 bones per vertex), `skin: Arc<SkinData>` (bindposes + per-submesh bind-pose CPU mesh, joints, weights; shared by every instance through `LoadedModel.skins`, keyed by (mesh, skin)) and `deformed` (per-instance meshes; `draws()` prefers them).
- `goat_animation::update_skinned_meshes(world, gfx, scratch)` skins them — from `AnimationPlugin::late_update` in play, and every frame on the editor's edit world (so edit mode shows the rest pose, not the raw bind pose):
  - skipped when `update_when_offscreen` is off and every camera looks away or is farther than its far plane (bounds sphere test);
  - palette = inverse(renderer world) × bone world × bindpose;
  - linear blend (the strongest `quality.bones()` weights, renormalized) into a scratch `MeshData`;
  - `update_mesh` on the deformed mesh, created on first use.
- The scene codec saves settings only (mesh, materials, root bone name, bone count, quality...); bones / skin come back from the model prefab, so loading onto a model's renderer updates its settings.

## Model import settings (goat-assets model_import.rs)
- `<model>.import.json` → `ModelImport` (Unity's ModelImporter, snake_case; missing keys = Unity's defaults). Read by `ModelImport::of` (an empty file = defaults).
- `load_model` = parse + `model_import::apply`, in this order: Scale Factor (node translations, vertices, bindpose translations, animated translations, camera / light distances), drop cameras / lights, Normals Calculate (`recalculate_normals`: corners grouped by position, faces within the Smoothing Angle averaged area-weighted, re-welded; skipped above 65 535 split vertices), animations (None / Import off → none; `clips` cut from takes by frame at `animation_rate`, ends resampled, Loop Time; Keyframe Reduction drops keys the neighbours' interpolation reproduces within the errors), Sort Hierarchy By Name, Preserve Hierarchy off (a single root without mesh / camera / light / joint / channel and with uniform scale disappears; its TRS is baked into its children and their channels).
- `AssetServer::model`: materials — None → `builtin:default`; Import → `remapped_materials[name]`, else External `materials/<name>.mat.json` if it exists, else the embedded `#material:i`. Read/Write keeps the CPU meshes.
- `spawn_model_into`: nodes in parent-first order from the (shaped) roots; cameras / lights become components; Optimize Game Objects (Generic) → `ModelSkeleton` on the root, only mesh / camera / light / exposed nodes get entities (under the root, at their rest world pose); then `on_model_spawned` hooks (goat-animation: Generic → `Animator::default()`, Legacy → `Animator::playing(first clip)`, unless one exists).
- goat-animation poses through `PoseTarget` (entities' Transforms or the skeleton's locals); after posing, `ModelSkeleton::update` computes worlds and moves the driven entities. Skinning with a `SkeletonRef`: bone world = root GlobalTransform × skeleton world.
- FBX: the converter applies Convert Units (`target_unit_meters`, 0 = none) and the bake rate, cameras / lights (axes right-handed Y-up = looking along -Z), and records the file unit in the GLB (`asset.extras.goat.fileUnit`) for the inspector. glTF cameras are core glTF; lights are KHR_lights_punctual read from the raw JSON (`raw_json`), written back by `write_glb`.
- Editor: `model_info` (nodes with paths / parents, meshes, materials, takes before clips, clips, file unit, size), `extract_model_materials` (PNG of decoded images, `.mat.json` via `MaterialFile::from_desc` with 8-bit colors, the remap), `reload_model` on a changed model / `.import.json`.

## Particles (goat-particles)
- `ParticleSystem` holds its settings and its particles (`Vec<Particle>`). Bookkeeping lives in `state: EmitterState` (xorshift RNG, cycle time, emission accumulator), so `..Default::default()` works outside the crate.
- `simulate(dt, world matrix)`:
  - emission: rate accumulator, plus bursts crossing their cycle time;
  - shape sampling in local space, transformed to world when `Space::World`;
  - gravity, drag, spin; particles die at their lifetime.
- Drawing: billboards facing the main display's lowest-depth camera, size / color lerped over life, `z` = −distance along the camera forward (back to front in the Batcher). Default texture: a 64² soft dot.

## UI (goat-ui)
- One UI camera per `Canvas`: ortho, height = display pixels, clear depth only, culling = `UI_LAYER` (30), depth 1000 + sorting order. It is spawned at a far origin per canvas; 1 unit = 1 pixel.
- `RectTransform::resolve(parent min, parent max)`:
  - anchors span = lerp of the parent rect;
  - size = span + `size`;
  - pivot at lerp(anchors, pivot) + `offset`.
  - Layout is depth-first from each canvas (children ordered by entity id). Rects go to the `UiLayout` resource.
- Drawing: in layout order, z increases by 0.001 per element, so the Batcher's z sort keeps hierarchy order. Images and texts go into one Batcher; texts are drawn through `goat_text::Fonts::quads` + `flush`.
- Input (`update`): the pointer (y-down screen pixels of the secondary display) is hit-tested against last frame's rects, topmost first.
  - `UiButton` gets press / hover / click (`clicked` only on the release frame, only when released over the pressed element) and tints its `UiImage`.
  - `UiToggle` flips; `UiSlider` follows the drag.

## Desktop backend (goat-gfx-wgpu)
- wgpu 25 with `Backends::VULKAN` on Windows / Linux; `GOAT_WGPU_BACKEND` overrides it. winit 0.30 `ApplicationHandler`.
- One uniform buffer per frame with dynamic offsets (256-byte alignment): per view, a clear color and frame uniforms; per item, object uniforms (model, normal matrix, colors).
- Each viewport is cleared by a fullscreen triangle with depth compare Always and depth write 1.0, so several cameras can share a display.
- Pipelines are cached by (blend, cull, depth_write). Shading is WGSL Blinn-Phong (`standard.wgsl`).
- Color space: an sRGB surface and `Rgba8UnormSrgb` textures; material / light / clear colors are linearized on upload.
- `request_screenshot(path)`: the surface is created with `COPY_SRC`, and the frame is read back to a PNG.

## 3DS backend (goat-gfx-citro3d)
- Initialisation:
  - `C3D_Init`
  - render targets: top SSAA 480×800, top left / right 240×400 (stereo), bottom 240×320; all RGBA8 + DEPTH24_STENCIL8
  - the shader (`picasso` in build.rs → `include_bytes!`)
  - the attribute layout, which matches `Vertex`
  - light env + the 8 hardware lights (enabled per view by `set_lights`)
- Per material (`set_material`): texenv stages.
  1. texture × vertex color (or vertex color alone)
  2. × fragment primary (ambient + diffuse from the light env), with alpha × the material's alpha (constant)
  3. + fragment secondary (specular)
  - Lit materials bind the light env and its `C3D_Material` (B, G, R order!), plus a cached specular LUT per shininess (`LightLut_FromArray`, x^n). Unlit materials unbind the light env.
  - Then cull, depth test (`GPU_GREATER`: citro3d projections use reversed depth, like its examples) and blending.
- Per frame:
  - The 3D slider picks mono SSAA (display transfer with `GX_TRANSFER_SCALE_XY`) or stereo (two eyes, `Mtx_PerspStereoTilt`, interocular = slider / 3).
  - Each display's views are drawn into its target. A display no view draws to is cleared black.
- Per view: projection (aspect of the viewport), depth map (W-buffer for perspective: linear depth), lights in view space (w = 0 directional, 1 positional; distance-attenuation LUT per range, spot LUT per cone; colors capped at 1.0), fog LUT per (near, far, start, end), partial viewport + clear quad.
- Per draw: the model-view matrix uniform, then `BufInfo` for the mesh VBO, then `C3D_DrawElements` (u16).
- Memory: VBO / IBO in `linearAlloc` memory, flushed with `GSPGPU_FlushDataCache`. Textures: `C3D_TexInitWithParams` (format, mip levels down to 8×8), each level converted + flipped + tiled by `tile_texture`, `C3D_TexLoadImage` per level.

## Tools: the `goat` CLI
- `Config`:
  - the workspace root: the nearest Cargo.toml with `[workspace]` and `goat-core`
  - tool paths: an env var, then `goat.local.toml`, taking the first path that exists
  - `out/<platform>/`
- `Builder` trait: `id`, `aliases`, `description`, `check(config) -> problems`, `build(config, options) -> artifact`, `run(config, artifact, args)`.
  - `desktop`: `cargo build -p goat-player-desktop`.
  - `3ds`: `cargo +nightly 3ds build -p goat-player-3ds` with `DEVKITPRO`, `DEVKITARM`, `PATH` and `LIBCLANG_PATH` set. `run` opens the configured emulator.

## Planned extension points
- Asset pipeline: importers run on the host and write platform formats. 3DS textures go through tex3ds formats; meshes become `Vertex` + u16 indices split to fit.
- Scripting hosts as `Behaviour` implementations. A C ABI (cbindgen) is shared by C / C++ and by the C# / TS bindings.
- More backends: wgpu covers web / Android / iOS; deko3d (Switch) and GX (Wii) will be new `GraphicsBackend` implementations.
