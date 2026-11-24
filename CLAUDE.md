# GoatEngine — notes for Claude

A native game engine in **Rust** (owner's choice over C++ for memory safety). The first target is the
**Nintendo 3DS** (citro3d / PICA200); desktop (wgpu, Vulkan) is the second target and the quick
preview of the 3DS's two screens. The goal is feature parity with the owner's PiaEngine
(F:\Workspace\ts\PiaEngine) — see [ROADMAP.md](ROADMAP.md). The architecture is in
[docs/architecture.md](docs/architecture.md); user docs are in [README.md](README.md) (Vietnamese).

## Working with the owner
- Reply in **Vietnamese**. README / ROADMAP are Vietnamese; code, comments and these notes are English.
- **No git repository yet and no commit permission**: don't `git init` or commit unless asked.
- Verify for real: a desktop screenshot (`--frames N --screenshot x.png`) and the 3DS build in an emulator (below), not only `cargo check`.
- Never touch the owner's apps / profiles: Citra at `E:\App\nightly` uses the owner's real `%APPDATA%\Citra` config — don't run tests with it or change its config; use a portable copy (below). Kill only processes you started.
- The owner prefers Unity's names and concepts (MonoBehaviour → `Behaviour`, Linear/Gamma color space, Animator...).

## Layout
```
crates/goat-core          math (glam re-export), Color (sRGB-authored, to_linear), Handle/Pool, log sink, Time
crates/goat-gfx           backend-neutral API: GraphicsBackend trait, MeshData/Vertex (48 B), TextureData, MaterialDesc,
                          RenderView (+ draw_order: opaque front→back, blended back→front), Display::Main/Secondary
crates/goat-runtime       Engine::frame (Game → Behaviours → transforms → views → gfx.render), hecs ECS components, Input
crates/goat-gfx-wgpu      desktop backend (WGSL Blinn-Phong, dynamic-offset uniforms, per-viewport clears, screenshots)
crates/goat-gfx-citro3d   3DS backend; ALL citro3d FFI is in src/pica.rs (cfg horizon); lib.rs = portable helpers (tile_rgba8)
crates/goat-sprites       2D: Sprite / SpriteRenderer, SpritePlugin batches sprites per texture into dynamic meshes
crates/goat-assets        AssetIo (romfs:/ on 3DS, assets/ next to exe), PNG, glTF/GLB, *.mat.json, AssetServer (keys <-> handles)
crates/goat-scene         scene / prefab JSON, SceneRegistry (component codecs, serde behaviours), load / instantiate / save
crates/goat-text          TTF fonts (fontdue) → dynamic glyph atlas, Text component + layout, batched through goat_sprites::Batcher
crates/goat-physics       Rapier 3D: RigidBody / Collider, fixed-step sync, collision events, raycast (Physics resource)
crates/goat-audio         WAV/OGG clips, software Mixer, AudioSource / AudioListener, outputs: cpal / ndsp (3DS) / WAV dump
crates/goat-animation     Animator (weighted glTF clips, crossfade) + CPU skinning into per-instance meshes
crates/goat-particles     ParticleSystem (rate / bursts / shapes / over-lifetime) → camera-facing quads via Batcher
crates/goat-ui            Canvas + RectTransform (Unity anchors) + Image / Text / Button / Toggle / Slider, pointer input, own camera per canvas
games/hello/assets        the sample's assets (Suzanne GLB exported by Blender, logo PNG, materials, prefabs)
crates/goat-platform-*    main loop + input mapping (winit / libctru)
players/*                 one-line binaries: platform::run(game)
games/hello               the sample game (depends only on goat-runtime)
tools/goat                CLI: goat doctor | build <platform> | run <platform>; builders/*.rs = one Builder per platform
tools/scripts             citra-shot.ps1 (emulator screenshot)
third_party               ctru-rs / citro3d-rs vendored (+ Windows bindgen fix), excluded from the workspace
```

## Commands (run builds through PowerShell: devkitPro / LLVM paths are Windows paths)
- Host build + tests: `cargo build` / `cargo test` (default-members = everything but the 3DS player; goat-gfx-citro3d builds on the host with only its portable parts).
- CLI: `cargo build --release -p goat`, then `target\release\goat.exe doctor | build desktop | build 3ds [--debug] | run desktop -- --layout 3ds`. Output in `out/<platform>/`.
- Tool paths: env vars or `goat.local.toml` (gitignored): `devkitpro`, `libclang`, `citra`. The devkitPro installer sets `DEVKITPRO=/opt/devkitpro` (MSYS path) globally — `Config::path` picks the first candidate that exists.
- Raw 3DS build: `$env:DEVKITPRO="E:\App\devkitPro"; $env:DEVKITARM="E:\App\devkitPro\devkitARM"; $env:PATH="E:\App\devkitPro\devkitARM\bin;E:\App\devkitPro\tools\bin;"+$env:PATH; $env:LIBCLANG_PATH="E:\App\LLVM\bin"; cargo +nightly 3ds build --release -p goat-player-3ds`.
- Desktop screenshot: `out\desktop\goat-hello.exe --layout 3ds --frames 150 --screenshot x.png` (frame timing is real time, so animated objects differ between runs).

## Verifying the 3DS build in Citra
- Citra's **OpenGL** renderer is broken on the owner's laptop (hybrid GPU, GL 4.3): the official devkitPro `fragment_light` example renders black and our draws crash `nvoglv64.dll`. Use **Vulkan**.
- Portable Citra (doesn't touch the owner's config): copy `E:\App\nightly\*.exe, *.dll` and `plugins\` into a scratch folder, create `user\` next to `citra.exe` (portable mode), run it once, then in `user\config\sdl2-config.ini` set `graphics_api = 2` (Vulkan) and `log_filter = *:Info Debug.Emulated:Debug` (shows the game's log).
- `tools/scripts/citra-shot.ps1 -Rom out\3ds\goat-hello.3dsx -Out shot.png -Citra <portable>\citra.exe` → screenshot + log (`shot.png.err`). PrintWindow flag 2 is what captures the GPU surface.
- If Citra vanishes: Windows Application event log (`Get-WinEvent ... 'Application Error'`) tells whether the emulator itself crashed. A Rust panic on 3DS shows on the bottom screen (goat-platform-3ds panic hook) and in the log.
- Bisect GPU problems with build-time flags: `$env:GOAT_3DS_DEBUG="nodraw,nodrawcall,unlit,notex,nossaa,vpfull,zbuffer,logvp"` (comma-separated, read by `option_env!` in pica.rs; cargo rebuilds when it changes).

## Gotchas (learned the hard way)
- rust3ds `-sys` build scripts build bindgen allowlists from host paths: on Windows the `\` separators broke the regexes → empty bindings and a missing `libctru_statics_wrapper.c`. The vendored copies use separator-agnostic regexes (`[/\\]`); keep the `[patch]` entries and `exclude = ["third_party"]` (the vendored crates inherit from their own workspace manifests).
- ctru-sys and citro3d-sys both generate `shaderProgram_s` / `shaderInstance_s`: same layout, distinct Rust types — cast pointers between them (pica.rs does).
- `C3D_Material` colors are stored **B, G, R** (a blue material rendered orange until swapped). `C3D_RenderTargetClear` takes 0xRRGGBBAA; texenv constant colors 0xAABBGGRR; C3D_FVec / C3D_Mtx rows are stored w z y x.
- `Mtx_PerspTilt` / `Mtx_OrthoTilt` already rotate for the sideways 3DS screens; render targets are created as (240, 400) / (480, 800 for SSAA) — width and height swapped.
- Textures on PICA: power of two, 8..1024, Morton-tiled 8×8, RGBA8 bytes in ABGR order (`tile_rgba8`).
- hecs components must be `Send + Sync`, hence `Behaviour: Send + Sync + 'static`. Scripts are removed from the entity while they run, which moves entities between archetypes: **ECS iteration order is not stable** — anything order-dependent (draw order!) must sort explicitly (`RenderView::draw_order`).
- Colors are sRGB-authored. The wgpu backend renders to an sRGB surface and linearizes material / light / clear colors (`c4(color, linear)`); the 3DS renders in gamma space with them as is. Vertex colors are not linearized yet.
- The citro3d backend keeps the shader program, light env, the 8 lights and every LUT in a `Box<Fixed>`: citro3d stores pointers to them, so they must never move — LUT caches are `HashMap<_, Box<Lut>>` (a HashMap moves its values when it grows).
- PICA textures are **bottom-up**: `tile_texture` flips rows (the first row in memory is v = 0). Engine UVs are top-left like wgpu; a marker texture (red top half) in the sample checks it.
- PICA light color registers are 8-bit and **wrap** above 1.0 (intensity 2 turned orange pink): `set_lights` divides the color by its largest channel. Lighting is gamma-space and clamps, so strong lights wash out to white sooner than on desktop (expected).
- Depth on 3DS: perspective views use the **W-buffer** (`C3D_DepthMap(false, -1/near, 0)`: depth = 1 near → 0 far, linear) — uniform precision and a linear fog LUT. With the default Z-buffer the whole fog range of a 0.1..200 camera fell into 1–2 LUT entries. Orthographic views use the Z-buffer map (already linear). PICA clip z is [-w, 0] with 0 = far: geometry at exactly z = 0 is clipped (the partial-clear quad sits at -1e-4).
- Partial viewports on 3DS (`C3D_SetViewport`, verified in Citra): GPU x = (1 − (vp.y + vp.h)) × target width, GPU y = (1 − (vp.x + vp.w)) × target height — both screen axes reversed. Partial views are cleared with a quad (depth Always), full ones with `C3D_RenderTargetClear`.
- Spot / distance-attenuation LUTs: distance attenuation through `LightLutDA_Create` with a Rust `extern "C"` callback (`window_attenuation`); spot cones as a signed LUT built by `lut_table(f, signed = true)` (entries 128..255 = negative inputs). The engine's light math (attenuation / spot smoothstep) lives in `goat_gfx::Light` and both backends use it.
- Comparing platforms: build with `GOAT_HELLO_FREEZE=1` (no animation, same frame on desktop and 3DS); `GOAT_HELLO_LIGHT_TEST=1` = no sun, a pure red point + pure green spot (checks light colors). They're `option_env!` in the game, so set them before `goat build`.
- Subsystems are crates exposing a `goat_runtime::Plugin`; games list them in `Game::plugins()` (only listed crates are in a build). Per-frame geometry goes through `update_mesh` + helper entities with a `MeshRenderer`.
- 3DS mid-frame clears: `C3D_RenderTargetClear` is a memory fill that may run before queued draws of earlier views on the same target — clear later views (depth only, partial) with the quad (`clear_viewport`), in command order.
- Owner's order of work (2026-11-24): finish every engine component first (2D, assets, scenes, physics, audio, animation, particles, UI, scripting, editor); platform builders (web...) come last.
- Citra (portable, Vulkan) can die at start-up with `vk::OutOfDeviceMemoryError` while allocating its 512 MB upload buffer when the machine is low on RAM (~2 GB free with the owner's Chrome open): it is the host, not the game — even the devkitPro `fragment_light` example dies then. The SDL frontend has no software renderer (graphics_api = 0 crashes). Retry later; never close the owner's apps.
- Asset keys always use `/`; `.json` assets may carry a UTF-8 BOM (stripped by `AssetIo::read_string`). Asset sample files are generated: `games/hello/assets/models/suzanne.glb` by Blender 3.0 headless (`blender -b --factory-startup --python make_suzanne.py -- out.glb`, factory settings = the owner's Blender config untouched), `textures/logo.png` by System.Drawing.
- Audio checks without ears: `GOAT_AUDIO_DUMP=x.wav` makes `AudioPlugin` mix on the main thread into a WAV file (rewritten each second) — read its RMS per window to see loops / one-shots / pan. A normal desktop run really plays through the speakers (cpal): keep test runs short.
- Text and sprites share `goat_sprites::{Quad, Batcher}`; dynamic textures use `GraphicsBackend::update_texture` (same size only). Font in the sample: Be Vietnam Pro (OFL, `games/hello/assets/fonts/OFL.txt`).
- Physics bodies are created from the Transform chain on the first fixed step after a Collider appears; the sample's `GOAT_HELLO_FREEZE` doesn't stop physics (screenshots with falling balls differ between runs).

## Editor (editor/)
- Tauri 2 shell (`editor/src-tauri`, crate `goat-editor`: file ops in `fsops.rs` with tests, commands in `main.rs`, watcher → `assets-changed`) + React UI (`editor/src`, Vite, dockview-react docking, layout saved in localStorage) + the engine as wasm (`editor/engine`, crate `goat-editor-engine`: `core.rs` portable + tested with NullBackend, `web.rs` wasm-bindgen API).
- Build the engine for the editor: `cargo build --target wasm32-unknown-unknown --release -p goat-editor-engine` then `wasm-bindgen --target web --out-dir editor/src/engine-pkg target/wasm32-unknown-unknown/release/goat_editor_engine.wasm` (wasm-bindgen-cli **0.2.130** = the crate version in Cargo.lock; regenerate after any engine change).
- Run (dev): `npm run dev` in `editor/` (Vite on 5190), then `target/debug/goat-editor.exe` (project = `--project <game dir>` or the sample `games/hello`). Release: `cargo build --release -p goat-editor` embeds `editor/dist` (`npm run build` first).
- Automation: start the editor with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333` and drive the page through CDP (Node's global WebSocket): `window.__goat` exposes getState / setState / handleDrop / api / engine() / openScene / startPlay / stopPlay. `editor/tests/e2e.mjs` = Project panel checks on a *copy* of a project.
- Gotchas: Chromium's `scrollIntoView()` now returns a promise — never return it from a React effect (it blanked the whole UI). Native drags (tauri-plugin-drag) can't be automated; drops are tested through `handleDrop`. The Viewport canvas must stay mounted (dockview `renderer: 'always'`): a WebGPU surface is bound to its canvas. Edit mode = `Engine::editing` (only plugins with `in_edit_mode()` run, `view_camera` = the editor camera); Play = a second Engine, dropped on Stop.
- hecs `Ref<T>` implements Clone: `world.get::<&T>(e).map(|r| r.clone())` clones the *borrow* (a later `get::<&mut T>` panics "already borrowed") — write `(*r).clone()`.
- `Resources::take::<T>()` returns `Box<T>`: give that Box back to `put_back` as is (wrapping it again stores a `Box<Box<T>>` under another TypeId and the resource silently disappears — the frozen "-- FPS" bug).
- Vertex colors are sRGB-authored: the wgpu shader linearizes them when the target is sRGB (`camera_forward.w` flag); sprites / UI / particles tint through vertex colors.
