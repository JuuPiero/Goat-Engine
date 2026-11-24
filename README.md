# GoatEngine

Game engine viết bằng **Rust**, chạy native để tận dụng toàn bộ tài nguyên máy: không GC, không runtime trung gian, đa luồng được. Kiến trúc chia lớp rõ ràng, nên thêm một nền tảng mới chỉ cần thêm *backend đồ họa + platform + builder*, không phải sửa lõi.

Nền tảng đầu tiên là **Nintendo 3DS** (GPU PICA200, qua citro3d). Desktop (Windows / Linux / macOS, qua wgpu + Vulkan) vừa là nền tảng thứ hai, vừa là chỗ xem trước nhanh hai màn hình 3DS.

> Trạng thái: **nền móng** (giai đoạn 0–1 của [ROADMAP.md](ROADMAP.md)). Đã có renderer 3DS + desktop, ECS, script `Behaviour`, input và CLI `goat` với builder cho 2 nền tảng. Mục tiêu bản đầu là ngang tính năng với PiaEngine; lộ trình nằm trong ROADMAP.

## Vì sao Rust

- An toàn bộ nhớ khi biên dịch: không có use-after-free hay data race kiểu C++. Đây là lý do chính để chọn Rust.
- Hiệu năng ngang C/C++, chạy được trên vi xử lý yếu (ARM11 268 MHz của 3DS) lẫn PC nhiều nhân.
- Cross-compile tốt: 3DS (`armv6k-nintendo-3ds`), desktop, sau này là Switch (`aarch64-nintendo-switch-freestanding`), web (wasm32), Android, iOS.
- FFI với C sạch sẽ: gọi libctru/citro3d, và cũng là cửa cho script C/C++ / C# / TypeScript (xem ROADMAP).

## Kiến trúc

```
goat-core            toán (glam), Color, Handle/Pool, log, Time — không phụ thuộc gì
   │
goat-gfx             API đồ họa trung lập: trait GraphicsBackend, MeshData/Vertex, TextureData,
   │                 MaterialDesc, RenderView/DrawItem, Display (Main / Secondary), draw_order()
   │
goat-runtime         Engine + ECS (hecs): Transform/GlobalTransform/Parent, Camera, MeshRenderer,
   │                 DirectionalLight, Input trừu tượng, Behaviour (script), trait Game
   │
   ├── backend đồ họa        goat-gfx-wgpu (Vulkan/Metal/DX12)   goat-gfx-citro3d (PICA200)
   ├── platform              goat-platform-desktop (winit)        goat-platform-3ds (libctru)
   └── player (binary)       players/desktop                      players/3ds
games/hello          game mẫu (chỉ phụ thuộc goat-runtime, chạy nguyên si trên mọi nền tảng)
tools/goat           CLI: goat build|run <platform>, goat doctor — mỗi nền tảng là một Builder
third_party/         ctru-rs + citro3d-rs (rust3ds) vendored, có bản vá bindgen cho Windows
```

Quy tắc phụ thuộc: lớp trên **không bao giờ** biết lớp dưới là nền tảng nào. Game chỉ thấy `goat-runtime`. Backend chỉ thấy `goat-gfx`. Mọi FFI nằm gọn trong backend/platform tương ứng (ví dụ toàn bộ FFI citro3d nằm trong `crates/goat-gfx-citro3d/src/pica.rs`).

### Một frame

`Engine::frame(dt, gfx)`:

1. `Game::start` (lần đầu)
2. `Game::update`
3. các `Behaviour` (`start` / `update`) của từng entity
4. tính `GlobalTransform` theo cây cha–con
5. mỗi `Camera` sinh ra một `RenderView` (display, viewport, view, projection, clear, ánh sáng, danh sách `DrawItem`)
6. `gfx.render(&views)`

Backend chỉ nhận dữ liệu thuần, không giữ trạng thái game.

### Hai màn hình

`Display::Main` là màn trên của 3DS (400×240, có 3D stereo) hoặc cửa sổ desktop. `Display::Secondary` là màn cảm ứng 320×240. Camera chọn display của nó. Desktop có `--layout 3ds` để xem cả hai màn như trên máy (Main 5:3 phía trên, Secondary 4:3 phía dưới). Trong layout đó, chuột đóng vai bút cảm ứng.

### Ánh sáng, fog

```rust
world.spawn((Transform::from_position(vec3(0.0, 1.0, 0.0)), PointLight { color: Color::hex(0xff8a2a), intensity: 1.4, range: 4.5 }));
world.spawn((spot_transform, SpotLight { spot_angle: 50.0, inner_spot_angle: 30.0, ..Default::default() }));
world.spawn((RenderSettings { ambient: Color::rgb(0.2, 0.22, 0.28), fog: Some(Fog { color: sky, start: 9.0, end: 24.0 }) },));
```

`DirectionalLight` / `PointLight` / `SpotLight` (góc tính bằng độ, giống Spot Angle / Inner Spot Angle của Unity). `RenderSettings` (ambient + fog) đặt trên một entity bất kỳ. Trên 3DS màu đèn tối đa là 1.0 mỗi kênh, nên đèn mạnh hơn sẽ được giữ nguyên tông màu và giảm xuống 1.0.

### Màu sắc

Màu (`Color::hex`, `Color::rgb`) được hiểu là **sRGB**, tức là chọn màu nào thì hiện đúng màu đó:

- **3DS** render trong gamma space nên giữ nguyên màu.
- **Desktop** render vào surface sRGB, nên màu material / clear / light được đổi sang linear trước, tương tự *Linear color space* của Unity.

Vì vậy hai nền tảng cùng màu gốc. Khác biệt duy nhất là lighting: desktop tính trong linear space nên các mặt tối tương phản hơn.

## Renderer 3DS (citro3d)

- **Lighting per-pixel bằng phần cứng**: fragment lighting của PICA200 (Lambert + Blinn-Phong) với **8 đèn**: directional, point (LUT suy giảm theo khoảng cách) và spot (LUT hình nón). Vertex shader xuất normal dạng quaternion + view vector; bảng LUT specular được tạo theo `shininess` và cache lại.
- **Fog** bằng khối fog của PICA. Depth dùng **w-buffer** nên tuyến tính: fog chính xác và độ chính xác depth đều từ gần tới xa.
- **Nhiều camera trên một màn hình**: viewport một phần (ví dụ picture-in-picture), clear riêng từng vùng.
- **Texture**: mipmap; định dạng RGBA8 / RGB565 / RGBA4 / RGBA5551 (16-bit tốn một nửa bộ nhớ và băng thông).
- **Khử răng cưa SSAA 2×2**: màn trên render ở 480×800 rồi thu nhỏ bằng display transfer (`GX_TRANSFER_SCALE_XY`). Khi bật thanh trượt 3D thì tự chuyển sang **stereo** (2 target 240×400, `Mtx_PerspStereoTilt`).
- Vertex/index buffer nằm trong linear memory. Texture được lật dọc (PICA đọc ảnh từ dưới lên) và tile Morton 8×8 khi upload (`tile_texture`, có test trên host).
- **Frustum culling** chạy ở runtime cho mọi backend: vật nằm ngoài tầm nhìn camera không được gửi xuống GPU. Mỗi view chỉ nhận 8 đèn quan trọng nhất (directional trước, rồi các đèn gần camera).
- Thứ tự vẽ: vật đục trước (gần → xa), sau đó vật trong suốt (xa → gần). Desktop dùng chung `RenderView::draw_order()` này.
- Chỉ đổi state khi material thay đổi.
- Shader PICA (`shaders/standard.v.pica`) được biên dịch bằng `picasso` trong `build.rs`, rồi nhúng vào binary.

## Cài đặt

| Nền tảng | Cần |
|---|---|
| Desktop | Rust stable (https://rustup.rs), driver Vulkan |
| 3DS | Rust nightly + `rust-src`, `cargo install --locked cargo-3ds`, devkitPro (nhóm `3ds-dev`), LLVM (libclang cho bindgen) |

Đường dẫn công cụ trên máy được đọc từ biến môi trường hoặc từ `goat.local.toml` ở thư mục gốc (file này không commit):

```toml
devkitpro = "E:/App/devkitPro"
libclang = "E:/App/LLVM/bin"
citra = "E:/App/nightly/citra.exe"   # emulator cho `goat run 3ds`
```

`goat doctor` sẽ báo còn thiếu gì và cách cài.

## Dùng CLI `goat`

```sh
cargo build --release -p goat          # build CLI (một lần)
target/release/goat doctor             # kiểm tra toolchain từng nền tảng
target/release/goat build desktop      # → out/desktop/goat-hello(.exe)
target/release/goat build 3ds          # → out/3ds/goat-hello.3dsx (+ .smdh)
target/release/goat run desktop -- --layout 3ds
target/release/goat run 3ds            # mở bằng emulator đã cấu hình
target/release/goat build 3ds --debug  # bản debug
```

Chạy trên máy 3DS thật: chép `out/3ds/goat-hello.3dsx` vào thư mục `/3ds/` trên thẻ SD rồi mở bằng Homebrew Launcher, hoặc gửi qua Wi-Fi bằng `3dslink`.

Tùy chọn của player desktop:

| Tùy chọn | Ý nghĩa |
|---|---|
| `--layout 3ds` | xem trước hai màn hình 3DS |
| `--size 1280x720` | kích thước cửa sổ |
| `--frames N` | tự thoát sau N frame |
| `--screenshot a.png` | chụp màn hình ở frame cuối |

### Phím trên desktop

| Nút 3DS | Phím |
|---|---|
| A / B / X / Y | K (hoặc Space) / J / I / U |
| L / R / ZL / ZR | Q / E / 1 / 3 |
| Start / Select | Enter / Backspace |
| D-pad | phím mũi tên |
| Circle pad | WASD |
| C-stick | TFGH |
| Thoát | Esc |

## Viết game

```rust
use goat_runtime::*;

struct Spinner { speed: f32 }
impl Behaviour for Spinner {
    fn update(&mut self, ctx: &mut Context, me: Entity) {
        let dt = ctx.time.delta;
        if let Some(mut t) = ctx.transform_mut(me) {
            t.rotation = math::Quat::from_rotation_y(self.speed * dt) * t.rotation;
        }
    }
}

pub struct MyGame;
impl Game for MyGame {
    fn start(&mut self, ctx: &mut Context) {
        let cube = ctx.gfx.create_mesh(&gfx::MeshData::cube(1.0)).unwrap();
        let mat = ctx.gfx.create_material(&gfx::MaterialDesc::default()).unwrap();
        ctx.world.spawn((Name("Cube".into()), Transform::default(), MeshRenderer::new(cube, mat),
                         Scripts::new().with(Spinner { speed: 1.0 })));
        // + một Camera và một DirectionalLight — xem games/hello
    }
}
```

Player của mỗi nền tảng chỉ có một dòng: `goat_platform_desktop::run(MyGame, ...)` / `goat_platform_3ds::run(MyGame)`.

## Subsystem = plugin (chỉ build những gì game dùng)

Mỗi hệ thống (2D, physics, audio, UI…) là một crate riêng có `Plugin`. Game khai báo plugin mình dùng; crate nào không khai báo thì không có trong bản build.

```rust
impl Game for MyGame {
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        vec![Box::new(goat_sprites::SpritePlugin::default())]
    }
    // ...
}
```

Thứ tự trong một frame:
1. `build` (một lần)
2. `fixed_update` (bước cố định 1/60 s)
3. `Game::update`, `Behaviour::update`, `Plugin::update`
4. `Behaviour::late_update`, rồi tính transform
5. `Plugin::late_update` (sinh dữ liệu vẽ)
6. render

### 2D: sprite (`goat-sprites`)

```rust
let coin = Sprite::grid(sheet, 64, 32, 16, 16, 0).with_pixels_per_unit(1.0);  // ô 0 của sprite sheet
world.spawn((Transform::from_position(pos), SpriteRenderer::new(coin).with_layer(5).with_sorting_order(1)));
```

`SpritePlugin` gom các sprite cùng texture, cùng layer và cùng sorting order thành một mesh, nên mỗi lô chỉ tốn một draw call. Thứ tự vẽ: sorting order trước, sau đó theo z từ xa đến gần.

Camera có `culling_mask` (layer), `clear` (màu) và `clear_depth` (giống "Depth only" của Unity: dùng cho HUD vẽ đè lên camera trước).

## Asset, scene, prefab

Thư mục `assets/` của game:
- Desktop: chép cạnh file exe (`goat build desktop` tự chép). Khi chạy thẳng từ `target/` thì dùng thư mục nguồn của game.
- 3DS: đóng gói vào romfs của .3dsx (`[package.metadata.cargo-3ds] romfs_dir`).

Asset được gọi bằng **key** là đường dẫn tương đối:
- `textures/logo.png`, `models/suzanne.glb`, `materials/logo.mat.json`;
- asset dựng sẵn: `builtin:cube` / `sphere` / `plane` / `default`;
- một phần của model: `models/a.glb#mesh:0:1`, `#material:2`.

| Loại | Định dạng |
|---|---|
| Ảnh | PNG. Thiết lập import đặt trong `<ảnh>.import.json`: `{ "mipmaps": true, "format": "rgb565", "filter": "nearest", "max_size": 256 }` |
| Model | glTF 2.0 / GLB: mesh, material, ảnh nhúng, cây node |
| Material | `*.mat.json`: `{ "base_color": "#3d8bff", "texture": "../textures/a.png", "shininess": 64, "blend": "alpha" }` |
| Scene / Prefab | `*.scene.json` / `*.prefab.json` (xem `crates/goat-scene/src/lib.rs`) |

```rust
fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
    vec![Box::new(AssetsPlugin::with_dev_root(concat!(env!("CARGO_MANIFEST_DIR"), "/assets"))), Box::new(ScenePlugin)]
}
// trong start:
goat_scene::registry(ctx).behaviour::<Spinner>("Spinner");          // behaviour lưu được vào scene (derive Serialize/Deserialize)
let shrine = goat_scene::instantiate(ctx, "prefabs/shrine.prefab.json", None)?;
let robot = goat_assets::spawn_model(ctx, "models/robot.glb", None)?;
goat_scene::load_scene(ctx, "scenes/level1.scene.json", LoadMode::Single)?;
let text = goat_scene::save_scene(ctx)?;                             // JSON của cả world
```

Trong file scene, mỗi entity có:
- `components`: Transform, Camera, MeshRenderer, các loại đèn, RenderSettings, SpriteRenderer…
- `scripts`: các behaviour đã đăng ký.
- `model` hoặc `prefab`: con của entity sinh ra từ file đó và **không** lưu lại vào scene. Sửa file prefab thì mọi scene dùng nó đều được cập nhật.

## Text, physics, audio

```rust
// Text: font TTF trong assets, tiếng Việt OK; size = chiều cao em theo đơn vị thế giới (HUD 1 px/đơn vị: pixel)
world.spawn((Transform::from_position(pos), Text::new("Xin chào!", "fonts/BeVietnamPro-Regular.ttf", 15.0).with_align(Align::Center, VAlign::Middle)));

// Physics (PhysicsPlugin): giống Rigidbody / Collider của Unity
world.spawn((Transform::from_position(vec3(0.0, 5.0, 0.0)), MeshRenderer::new(ball, mat),
             RigidBody::dynamic(), Collider::new(Shape::Sphere(0.5)).with_restitution(0.6)));
for e in &goat_physics::physics(ctx).events { /* Enter / Exit / TriggerEnter / TriggerExit */ }
let hit = goat_physics::physics(ctx).raycast(origin, Vec3::NEG_Y, 100.0);

// Audio (AudioPlugin): WAV / OGG từ assets hoặc âm tự tạo
goat_audio::play_one_shot(ctx, "sfx/coin.wav", 0.5);
goat_audio::play_at(ctx, "bonk", position, 1.0, 1.0);                       // âm 3D
world.spawn((Transform::default(), AudioSource::new("music.ogg").looping().play_on_start()));
// AudioListener đặt trên camera
```

- Collider không có RigidBody là vật tĩnh. Kích thước collider nhân với scale của transform.
- Đặt `RigidBody::velocity` hoặc dời Transform từ script thì physics nhận ngay ở bước sau.
- Audio: desktop phát ra loa mặc định (cpal), 3DS phát qua DSP. Máy 3DS thật cần file `/3ds/dspfirm.cdc` trên thẻ SD, Citra thì không cần.
- `GOAT_AUDIO_DUMP=out.wav` ghi âm thanh đã mix ra file thay vì phát ra loa (dùng khi test).

## Animation, particles, UI

```rust
// Animation: trên root của model glTF (spawn_model hoặc "model" trong scene)
let mut anim = Animator::playing("Sway");
anim.add("Hop", 1.0);                 // phát thêm clip (node khác cùng chạy, node trùng thì trộn theo trọng số)
anim.crossfade("Run", 0.3);           // chuyển clip mượt
world.insert_one(robot, anim);        // mesh có xương được skin bằng CPU, mọi nền tảng

// Particles
world.spawn((Transform::from_position(pos), ParticleSystem { rate: 40.0, shape: Shape::Cone { angle: 12.0, radius: 0.1 },
    color_start: Color::rgba(1.0, 0.6, 0.2, 1.0), color_end: Color::rgba(1.0, 0.1, 0.0, 0.0), gravity: -0.15, ..Default::default() }));

// UI trên màn cảm ứng: Canvas + RectTransform + Image / Text / Button / Toggle / Slider
let canvas = world.spawn((Transform::default(), Canvas::new(Display::Secondary)));
let btn = world.spawn((Parent(canvas), Transform::default(), RectTransform::anchored(vec2(0.5, 0.5), Vec2::ZERO, vec2(120.0, 30.0)),
                       UiImage::color(Color::hex(0x3a4152)), UiButton::default()));
if world.get::<&UiButton>(btn)?.clicked { /* ... */ }
```

Thêm vào game bằng `AnimationPlugin`, `ParticlePlugin`, `UiPlugin`. `RectTransform` dùng quy ước của Unity: đơn vị pixel, y hướng lên, anchor tính từ góc dưới trái.

## Editor

Editor dùng Tauri 2 + React; engine chạy bên trong editor dưới dạng WebAssembly và render bằng WebGPU.

- **Dock panel**: kéo tab để sắp xếp, chia đôi, ghép nhóm. Layout được nhớ lại; nút *Reset Layout* đưa về mặc định (Hierarchy | Scene/Game | Inspector, bên dưới là Project + Console).
- **Project (Asset Panel)**: dùng như một trình quản lý file.
  - Cây thư mục, breadcrumb, lưới có thumbnail, tìm kiếm.
  - Chọn nhiều (Ctrl/Shift/kéo khung), F2 đổi tên, Delete xóa vào Thùng rác, Ctrl+D/C/X/V, menu chuột phải.
  - Kéo thả qua drag gốc của hệ điều hành: vào thư mục (giữ Ctrl để copy), sang cửa sổ editor khác (*New Window*), ra Explorer hoặc ứng dụng khác; kéo file từ ngoài vào để import.
  - Đổi tên hay di chuyển asset thì tham chiếu trong scene, prefab, material được tự sửa theo.
- **Scene view**: double-click file `.scene.json` để mở.
  - Chuột phải hoặc Alt + chuột trái: xoay; chuột giữa: kéo khung nhìn; cuộn: zoom; F: căn vào vật đang chọn.
  - Click để chọn; Hierarchy và Inspector (Transform, kéo nhãn X/Y/Z để chỉnh) đồng bộ theo. Ctrl+S lưu scene.
- **Play (▶)**: chạy game thật (behaviour, physics, sprite, text) trong Game view, có thể bật layout 2 màn hình 3DS. Phím điều khiển giống bản desktop. Esc hoặc ■ để dừng; scene đang edit không bị thay đổi.

```sh
cargo build --target wasm32-unknown-unknown --release -p goat-editor-engine
wasm-bindgen --target web --out-dir editor/src/engine-pkg target/wasm32-unknown-unknown/release/goat_editor_engine.wasm
cd editor && npm install && npm run dev     # Vite dev server
cargo build -p goat-editor && target/debug/goat-editor.exe   # (thư mục gốc repo)
```

## Thêm một nền tảng mới

1. `crates/goat-gfx-<api>`: implement `goat_gfx::GraphicsBackend`.
2. `crates/goat-platform-<name>`: vòng lặp chính + map input về `goat_runtime::Input`.
3. `players/<name>`: binary gọi `run(game)`.
4. `tools/goat/src/builders/<name>.rs`: implement `Builder` (`check` / `build` / `run`), rồi đăng ký trong `builders::all()`.

Lõi và game không phải sửa gì.
