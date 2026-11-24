# GoatEngine

Game engine viết bằng **Rust**, chạy native để tận dụng toàn bộ tài nguyên máy: không GC, không runtime trung gian, đa luồng được. Kiến trúc chia lớp rõ ràng, nên thêm một nền tảng mới chỉ cần thêm *backend đồ họa + platform + builder*, không phải sửa lõi.

Nền tảng đầu tiên là **Nintendo 3DS** (GPU PICA200, qua citro3d). Desktop (Windows / Linux / macOS, qua wgpu + Vulkan) vừa là nền tảng thứ hai, vừa là chỗ xem trước nhanh hai màn hình 3DS.

> Trạng thái: renderer 3DS + desktop, asset / scene / prefab, text, physics 2D + 3D, audio, animation (có Animator Controller), particles, UI, input actions, script **Rust / TypeScript / JavaScript / C# / C / C++**, editor (Tauri) và CLI `goat` với builder cho 2 nền tảng. Mục tiêu bản đầu là ngang tính năng với PiaEngine; phần còn lại nằm trong [ROADMAP.md](ROADMAP.md).

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
goat-sprites / -assets / -scene / -text / -physics / -physics2d / -audio / -animation / -particles / -ui / -input / -tween / -save
                     subsystem = plugin, game chọn cái nó dùng (bản build chỉ chứa những gì game dùng)
goat-script (+ -js, -native)   script: lõi chung + TypeScript / JavaScript / C# (host JS) + C / C++ (C ABI)
games/hello          game mẫu (chạy nguyên si trên mọi nền tảng)
tools/goat-cs        C# → JavaScript (Roslyn, .NET)
tools/unity-goat-exporter  Editor tool của Unity: ParticleSystem → prefab GoatEngine
editor/              editor Tauri 2 + React; engine trong editor là wasm (editor/engine)
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
- **Texture**: mipmap; định dạng RGBA8 / RGB565 / RGBA4 / RGBA5551 / ETC1 / ETC1A4 (ETC1 nén khối 4 bit/pixel trên 3DS; 16-bit tốn một nửa bộ nhớ và băng thông).
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

- `SpriteMask` (như Unity): hình chữ nhật `size` (đơn vị thế giới, nhân theo scale; 0 = theo sprite của chính nó) + `center`, tuỳ chọn `sorting_range`. SpriteRenderer có `mask_interaction`: `VisibleInsideMask` (chỉ hiện phần trong mask) / `VisibleOutsideMask` (khoét phần trong mask). Cắt chính xác cả UV, chạy trên mọi backend kể cả 3DS; sprite bị xoay thì ẩn / hiện cả cái theo tâm.
## Asset, scene, prefab

Thư mục `assets/` của game:
- Desktop: chép cạnh file exe (`goat build desktop` tự chép). Khi chạy thẳng từ `target/` thì dùng thư mục nguồn của game.
- 3DS: đóng gói vào romfs của .3dsx (`[package.metadata.cargo-3ds] romfs_dir`).

Asset được gọi bằng **key** là đường dẫn tương đối:
- `textures/logo.png`, `models/suzanne.glb`, `materials/logo.mat.json`;
- asset dựng sẵn: mesh `builtin:cube` / `sphere` / `capsule` / `cylinder` / `plane` / `quad` (kích thước như Unity), material `builtin:default`, texture `builtin:white`;
- một phần của model: `models/a.glb#mesh:0:1`, `#material:2`;
- theo GUID: `guid:<32 ký tự hex>` (và `guid:...#mesh:0:1`). Mỗi asset có file `<tên file>.meta` = `{ "guid": "..." }`, do editor tạo và giữ nguyên khi đổi tên / di chuyển, như Unity. Nên commit cả file `.meta`.

Đổi tên hay di chuyển asset **bên ngoài editor** (Explorer, `git mv`) cũng không làm vỡ tham chiếu. Editor giữ bảng guid → đường dẫn trong `.goat/guids.json` (cạnh `assets/`, không commit, giống thư mục Library của Unity). Khi thấy một GUID đã sang đường dẫn khác, editor sửa mọi tham chiếu tới đường dẫn cũ và báo trong Console. File bị dời đi mà bỏ lại `.meta` thì editor nhận lại `.meta` cùng tên file. File chép kèm `.meta` thì bản chép nhận GUID mới.

| Loại | Định dạng |
|---|---|
| Ảnh | PNG, JPEG, KTX 1.1 (ETC1 nén sẵn — 3DS nạp thẳng các khối, máy khác giải mã; hoặc RGBA8 / RGB8). Tạo KTX: `cargo run -p goat-assets --example to_ktx -- in.png out.ktx`. Thiết lập import đặt trong `<ảnh>.import.json`: `{ "mipmaps": true, "format": "rgb565", "filter": "nearest", "max_size": 256 }` |
| Model | glTF 2.0 / GLB: mesh, material, ảnh nhúng, cây node |
| Material | `*.mat.json`: `{ "base_color": "#3d8bff", "texture": "../textures/a.png", "shininess": 64, "blend": "alpha" }` |
| Scene / Prefab | `*.scene.json` / `*.prefab.json` (xem `crates/goat-scene/src/lib.rs`) |
| Thiết lập project | `project.json` ở gốc `assets/`: `name`, `version`, `scenes` (Build Settings: `[{ "path": "scenes/menu.scene.json", "enabled": true }]`), `fixed_timestep`, `max_delta`, `gravity`, `gravity_2d`, `tags`, `layers` (`goat_runtime::ProjectSettings`, là một resource) |

Chuyển scene từ script giống Unity: `SceneManager.loadScene(1)` (chỉ số trong Build Settings), `SceneManager.loadScene("level1")` (tên file) hoặc theo key; tham số thứ hai `LoadSceneMode.Additive` để nạp thêm. Scene được nạp ở cuối frame. `SceneManager.getActiveScene()` trả `{ name, path, buildIndex }`; `sceneCountInBuildSettings`. Từ Rust: `goat_scene::request_scene(ctx, "1", LoadMode::Single)`. `goat build` nhận `GOAT_ASSETS` (thư mục assets khác) và `GOAT_START_SCENE` (scene khởi động) — nút Build của editor đặt sẵn cả hai.

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
- `active: false`: object bị tắt (Unity `SetActive(false)`): nó và mọi object con không được vẽ, không chạy script / behaviour, không có trong physics, UI, particle, âm thanh, navigation. Rust: `goat_runtime::set_active(world, e, false)`, `active_self`, `active_in_hierarchy`; script: `go.setActive(false)`, `go.activeSelf`, `go.activeInHierarchy`. Editor: checkbox cạnh tên trong Inspector, dòng mờ trong Hierarchy.
- `tag`: `Player`, `Enemy`…; `goat_runtime::find_with_tag`, script `go.tag`, `go.compareTag(t)`, `GameObject.findWithTag` / `findGameObjectsWithTag`; editor: ô Tag (gợi ý các tag có sẵn của Unity, gõ tag mới được).
- `model` hoặc `prefab`: con của entity sinh ra từ file đó và **không** lưu lại vào scene. Sửa file prefab thì mọi scene dùng nó đều được cập nhật.
- `overrides` (chỉ với `prefab`): thay đổi các entity **bên trong** prefab, như property override của Unity:
  ```json
  { "id": 5, "prefab": "prefabs/gate.prefab.json", "overrides": [
      { "target": [3], "name": "Cột phải" },
      { "target": [2, 1], "components": { "Transform": { "position": [0, 2, 0] }, "PointLight": null } } ] }
  ```
  `target` là đường id vào prefab: `[3]` là entity 3 của file prefab, `[2, 1]` là entity 1 của prefab lồng trong entity 2. `components` là JSON merge patch: object được gộp, giá trị khác bị thay, `null` xóa trường / component. `scripts` (nếu có) thay cả danh sách behaviour. Editor tự ghi `overrides` khi bạn sửa một entity con của prefab rồi lưu scene.
- `parent_target`: entity thêm làm con của một entity bên trong prefab, ví dụ `"parent": 5, "parent_target": [2, 1]`.

**Prefab variant** là file prefab có `base`: lấy các entity của prefab gốc, áp `overrides`, rồi thêm `entities` của chính nó. Id của entity thêm không được trùng id của prefab gốc, nhưng có thể dùng id của prefab gốc làm `parent`:
```json
{ "version": 1, "base": "prefabs/post.prefab.json",
  "overrides": [ { "target": [2], "components": { "Transform": { "position": [0, 3, 0] } } } ],
  "entities": [ { "id": 10, "name": "Flag", "parent": 1 } ] }
```
### Material: các hiệu ứng đẹp mà nhẹ

Các trường dưới đây ghi được trong `*.mat.json`, chạy được trên cả desktop lẫn 3DS (trên 3DS dùng phần cứng của PICA):

| Trường | Hiệu ứng |
|---|---|
| `emissive` + `emissive_texture` | Tự phát sáng theo một texture |
| `env_map` (`builtin:sky` hoặc `.cubemap.json`) + `reflectivity` | Phản chiếu cubemap |
| `alpha_cutoff` (+ `alpha_dither`, chỉ desktop) | Cắt lỗ theo alpha (lá cây, hàng rào) |
| `toon_steps` | Cel shading (ánh sáng chia bậc) |
| `rim_color` + `rim_power` | Viền sáng ở mép (fresnel) |
| `outline_width` + `outline_color` | Viền đen kiểu hoạt hình |
| `procedural`: `{ "kind": "water" \| "fire" \| "noise", "color_a", "color_b", "scale", "speed" }` | Texture tự sinh có chuyển động (nước, lửa) |

Thêm vào đó:
- `RenderSettings.ambient_ground`: ambient bán cầu (trên là màu trời, dưới là màu đất).
- `BlobShadow`: bóng tròn mềm dưới vật.
- `StaticBatching { bake_lighting }`: gộp vật tĩnh và nướng ánh sáng vào vertex.
- `LodGroup`: tự chọn mesh theo kích thước vật trên màn hình.

## Text, physics, audio

Chữ có thể có viền (`.with_outline(màu, 0.06)`), bóng (`.with_shadow(màu, [0.05, 0.05])`) và rich text (`.rich()`: `<color=#ff8800>`, `<size=1.5>`, `<b>`, `<i>`). Trong scene, các key tương ứng là `rich` / `outline` / `shadow` của `Text` và `UiText`.

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

### Audio: stream, mixer group

- Ogg dài hơn 512 KB (hoặc có `"load_type": "streaming"` trong `<clip>.import.json`) được giải mã dần khi phát, không bung hết ra RAM. Nhạc nền trên 3DS nên dùng cách này.
- Mixer group như AudioMixer của Unity. Khai báo trong `audio/mixer.json`:

  ```json
  { "groups": [ { "name": "Music", "volume": 0.8 }, { "name": "SFX" }, { "name": "UI", "parent": "SFX" } ] }
  ```

  `AudioSource { output: "Music", .. }` cho âm thanh đi qua group đó. Mỗi group có `volume`, `mute`, `lowpass` (Hz, làm tiếng nghe như dưới nước / khi pause).
- Trong script:
  - `Audio.playOneShot("sfx/coin.wav", 1, "SFX")` phát một lần.
  - `AudioSource.of(go).play() / pause() / unPause() / stop()`, đọc `isPlaying`, `time`.
  - `AudioMixer.set("Music", { lowpass: 600 })`, `AudioMixer.setVolumeDb("Music", -10)`.

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

UI còn có (tên giống Unity, đều dùng được trong file scene):
- `UiImage` kiểu `Sliced` (9-slice: `border` giữ nguyên kích thước, phần giữa giãn ra) và `Filled` (`fill_amount`, cho thanh máu / tiến trình).
- `UiLayoutGroup` (Horizontal / Vertical / Grid: padding, spacing, căn lề, điều khiển và giãn kích thước con), `UiLayoutElement`, `UiContentSizeFitter`.
- `UiMask`: cắt mọi thứ con nằm ngoài khung.
- `UiScrollRect`: kéo bằng tay có quán tính, bật lại khi kéo quá mép; con tên "Content" là phần cuộn.
- `UiInputField`: con "Text" và "Placeholder"; giới hạn ký tự, kiểu số nguyên / số thực / mật khẩu. Desktop gõ phím, trên 3DS chạm vào là mở bàn phím hệ thống.
- `UiDropdown`: con "Label"; danh sách mở đè lên mọi thứ khác.
- `UiRawImage`: texture vẽ nguyên dạng với `uv_rect` (lặp texture, render texture).
- `UiCanvasGroup`: `alpha` nhân cho cả cây con; `interactable` / `blocks_raycasts` tắt tương tác của cả cây (`ignore_parent_groups`).
- `UiAspectRatioFitter`: giữ tỉ lệ khung (WidthControlsHeight, HeightControlsWidth, FitInParent, EnvelopeParent).
- `UiToggleGroup`: các toggle nằm dưới nó chỉ một cái được bật (`allow_switch_off`).
- `UiScrollbar`: con "Handle", `value` / `size` / `direction`; điền tên ScrollRect vào `scroll_rect` để kéo thanh cuộn danh sách và ngược lại.
- `UiRectMask2D`: cắt cây con theo khung, không vẽ gì (như `UiMask` không hiện graphic).
- `UiShadow` / `UiOutline`: bóng đổ / viền cho Image, RawImage, Text (khoảng cách tính bằng pixel).
- `UiEventSystem` (`navigation: true`): D-pad / cần analog di chuyển vùng chọn, A để bấm, B để thoát ô nhập / đóng danh sách. Vùng chọn có màu riêng.

### Animator Controller

State machine như Mecanim của Unity, lưu trong file `.controller.json` (tạo bằng *Create ▸ Animator Controller*, sửa trong cửa sổ **Animator** của editor):

- **Tham số** `float` / `int` / `bool` / `trigger` (trigger tự tắt khi một transition dùng nó).
- **State**: một clip của model, hoặc *blend tree 1D* (nhiều clip trộn theo một tham số float, chạy đồng bộ thời gian). `speed` × tham số nhân tốc độ, `loop`.
- **Transition**: từ một state hoặc từ `Any State`, điều kiện (`if` / `ifnot` / `greater` / `less` / `equals` / `notequal`), *Has Exit Time* (thời gian chuẩn hoá), `duration` (giây crossfade).
- **Layer**: layer 0 tạo dáng, các layer sau ghi đè theo `weight`.

```rust
let mut anim = Animator::from_controller_asset(&assets, "animators/tentacle.controller.json")?;
anim.set_float("energy", 0.7);
anim.set_trigger("poke");
if anim.current_state(0) == Some("Poke") { /* ... */ }
```

Trong scene: `"Animator": { "controller": "animators/tentacle.controller.json", "parameters": { "energy": 0.5 } }`. Script gọi qua `gameObject.call("Animator.setTrigger", "poke")`.

### Physics: layer, mesh collider, joint, CharacterController

- **Layer:** `Collider { layer: 3, .. }` + `physics.set_layer_collision(0, 3, false)` (Layer Collision Matrix của Unity). `raycast_layers(..., mask)` và `overlap_sphere` lọc theo layer mask.
- **Mesh collider:** `Shape::Mesh { source: "models/rock.glb", convex: false }`.
  - `source` có thể là `builtin:cube`, một model, một mesh con `#mesh:0:0`, hoặc để trống để lấy mesh của MeshRenderer.
  - Mesh lõm chỉ dùng cho vật tĩnh / kinematic; `convex: true` dùng bao lồi, vật dynamic cũng được.
- **Joint:** đặt trên vật thứ nhất, `connected` là tên vật kia (để trống = gắn vào thế giới).
  - 3D: `Joint::hinge(Vec3::Z).to("Door Frame")` (có giới hạn góc, motor), ngoài ra có fixed / ball / slider / spring / rope.
  - 2D: `Joint2D` với fixed / hinge / slider / spring / distance.
- **CharacterController** (như Unity): `physics.move_character(world, e, velocity * dt, dt)` trượt theo tường, leo bậc (`step_offset`), đi dốc đến `slope_limit`, rồi cập nhật `is_grounded` / `collision_flags`.
- **Trong script:**
  - `Physics.raycast(origin, dir)` trả về `{ gameObject, point, normal, distance }`. Cũng có `Physics.overlapSphere`, `Physics.collisions(go)` (enter / exit / trigger của frame này), `Physics.addForce / addImpulse`.
  - `Physics.moveCharacter(go, motion)` là `controller.Move` của Unity; `Physics2D.*` có các lệnh tương tự cho 2D.

### Particles nâng cao

Các module tuỳ chọn (không ghi = tắt):
- `color_over_life` (gradient `[[t, "#rrggbbaa"], ...]`), `start_colors` (random giữa hai màu).
- `size_curve` / `speed_curve` (`[[t, v], ...]`), `velocity_over_life`.
- `texture_sheet` (`tiles`, `cycles`, `random_row`).
- `collision`: `planes` là danh sách `[điểm, pháp tuyến]`; `world: true` va chạm collider 3D, cần feature `physics`. Có `bounce`, `dampen`, `lifetime_loss`.
- `sub_emitters` (`on: birth | death | collision`, `system`: tên entity có system con).
- `trail` (dải mỏng hướng về camera).

Hiệu ứng làm sẵn trong Unity có thể chuyển sang bằng `tools/unity-goat-exporter` (xem README trong thư mục đó).

### Animation nâng cao

- **Avatar mask:** `"mask": ["Spine"]` trên một layer, layer đó chỉ điều khiển node `Spine` và các con của nó.
- **Layer additive:** `"blending": "additive"` cộng phần thay đổi của clip (so với frame đầu) lên tư thế hiện tại.
- **Blend tree 2D:** `"type": "2d"` với `parameter` + `parameter_y`; mỗi con có `position: [x, y]`. Trộn bằng gradient band, giống Freeform Cartesian của Unity.
- **Animation event:** `"events": { "Walk": [ { "time": 0.3, "name": "footstep" } ] }` (giây tính từ đầu clip), khai báo trong controller hoặc trong `Animator` của scene.
  - Rust: đọc `animator.fired`.
  - Script: `Animator.of(go).onEvent(e => ...)`.
- **Sprite flipbook** (`SpriteAnimator`, đặt cạnh `SpriteRenderer`):

  ```json
  "SpriteAnimator": { "play": "walk", "clips": { "walk": { "grid": [16, 16], "frames": [0, 1, 2, 3], "fps": 10 },
                                                 "hit": { "grid": [16, 16], "frames": [8, 9], "fps": 12, "loop": false, "next": "walk" } } }
  ```

  Trong script: `SpriteAnimator.of(go).play("hit")`.

## Line, trail (`goat-lines`)

`LineRenderer` (đường qua danh sách điểm) và `TrailRenderer` (vệt phía sau một vật đang chạy), giống Unity:
- Line: `positions` (toạ độ thế giới hoặc cục bộ, `use_world_space`), `loop`, `start_width` / `end_width` × `width_multiplier`, `start_color` / `end_color`.
- Trail: `time` (số giây một điểm tồn tại), `min_vertex_distance`, `emitting` (tắt thì vệt rời khỏi vật và mờ dần), `autodestruct` (xoá vật khi đứng yên và vệt đã hết); `clear()`.
- Chung: `alignment` `View` (quay về camera) / `TransformZ` (nằm phẳng theo trục Z của vật), `texture_mode` `Stretch` / `Tile`, `texture`, `blend` (Alpha / Additive), `layer`, `sorting_order`.
- Vẽ unlit, gom chung với bộ gom sprite (mỗi texture một draw call), nên chạy cả trên 3DS. Line hiện cả trong Edit Mode; trail chỉ ghi điểm khi Play.

## Navigation (`goat-navigation`)

NavMesh giống package AI Navigation của Unity (cùng thiết kế với `com.pia.ai-navigation` của PiaEngine):
- `NavMeshSurface`: bấm **Bake** trong Inspector (hoặc `bake_on_awake`, `build_nav_mesh()` lúc chạy). Hình học lấy từ collider của goat-physics (Box / Sphere / Capsule / Mesh) hoặc từ MeshRenderer (cả mesh trong model glTF). Có bán kính / chiều cao / độ dốc / bậc leo của agent, All / Volume / Children, layer mask, voxel size, min region area. Chọn surface trong editor để thấy lưới navmesh phủ lên Scene view. Dữ liệu bake lưu trong scene.
- `NavMeshAgent`: `set_destination` (đường được tính ở lần update kế tiếp: `path_pending`), tự tăng tốc / quay / phanh, `stopping_distance`, tránh agent khác theo `avoidance_priority`, `warp` / `move_by` / `reset_path`, `is_stopped`, đi một phần đường khi đích không tới được và tự tính lại khi navmesh đổi.
- `NavMeshObstacle` (Box / Capsule): agent lách quanh; bật `carve` thì khoét lỗ trên navmesh. `NavMeshModifier`: đổi area hoặc bỏ khỏi bake.
- Resource `NavMeshes` (``NavMesh`` của Unity): `calculate_path`, `sample_position`, `raycast`, `find_closest_edge`, tên area + cost.
- Script: `NavMeshAgent.of(go).setDestination(p)`, `.remainingDistance`, `.pathPending`, `.velocity`, `.isStopped`, `.warp()`… và `NavMesh.samplePosition(p, 2)` / `calculatePath(a, b)` / `raycast(a, b)` trong TypeScript / JavaScript / C#.
- Demo: `scenes/navigation.scene.json` (desktop: `goat-hello --scene scenes/navigation.scene.json`; 3DS: build với biến môi trường `GOAT_START_SCENE=scenes/navigation.scene.json`).
## Physics 2D

Crate riêng `goat-physics2d` (Rapier 2D): game chỉ 2D thì không mang physics 3D theo, và ngược lại.

```rust
world.spawn((Transform::from_position(vec3(0.0, 4.0, 0.0)), Rigidbody2D::dynamic(), Collider2D::circle(0.5).with_restitution(0.4)));
world.spawn((ground_transform, Collider2D::unit_box()));               // collider không có body = tĩnh
let hit = goat_physics2d::physics2d(ctx).raycast(origin, Vec2::NEG_Y, 20.0);
```

- Vật nằm trên mặt phẳng XY: mô phỏng đọc / ghi X, Y và góc quay quanh Z; độ sâu Z và các trục quay khác giữ nguyên.
- Shape: `box`, `circle`, `capsule`, `polygon` (bao lồi). Trigger, ma sát, độ nảy, mật độ hoặc khối lượng.
- `Physics2D`: `gravity`, `events` (Enter / Exit / TriggerEnter / TriggerExit), `raycast`, `overlap_point`, `apply_impulse`.
- Scene mẫu: `scenes/physics2d.scene.json`. Trong scene: `"Rigidbody2D": {}`, `"Collider2D": { "shape": { "circle": 0.5 } }`.
- Lực và effector (như Unity): `ConstantForce2D` (lực theo trục thế giới / theo hướng của body, mô-men); `AreaEffector2D` (gió / dòng chảy trong vùng collider: góc, độ lớn, dao động, lực cản); `PointEffector2D` (hút khi âm, đẩy khi dương; Constant / InverseLinear / InverseSquared); `BuoyancyEffector2D` (nước: `surface_level`, mật độ > 1 thì vật nổi, lực cản, dòng chảy); `TargetJoint2D` (kéo vật về một điểm như kéo bằng ngón tay). Effector tác động lên body động có tâm nằm trong collider của nó (thường là trigger).
- `PlatformEffector2D`: platform một chiều (nhảy xuyên từ dưới lên, đứng được ở trên; `surface_arc`, `rotational_offset`, `use_one_way`). `SurfaceEffector2D`: băng chuyền, đẩy vật đang chạm theo trục X của collider (`speed`, `speed_variation`).
- Joint 2D thêm: `wheel` (bánh xe: lò xo giảm xóc theo `suspension_angle`, `frequency`, `damping_ratio`, motor), `friction` (ma sát giữ vật lại với lực / mô-men tối đa, kiểu sàn nhìn từ trên xuống), `relative` (giữ hai vật lệch nhau `linear_offset` / `angular_offset` với lực tối đa).
- 3D: `ConstantForce` (`force`, `relative_force`, `torque`, `relative_torque`).

## Input actions

Giống Input System của Unity. File `.inputactions.json`:

```json
{ "maps": [ { "name": "Player", "actions": [
    { "name": "Move", "type": "value", "processors": "stickDeadzone(min=0.12,max=0.95)", "bindings": [
        { "path": "<Gamepad>/leftStick" },
        { "composite": "2DVector", "up": "<Gamepad>/dpad/up", "down": "<Gamepad>/dpad/down", "left": "<Gamepad>/dpad/left", "right": "<Gamepad>/dpad/right" } ] },
    { "name": "Jump", "type": "button", "bindings": [ { "path": "<Gamepad>/buttonSouth" }, { "path": "<Touchscreen>/press" } ] },
    { "name": "Charge", "type": "button", "interactions": "hold(duration=0.5)", "bindings": [ { "path": "<Gamepad>/buttonWest" } ] }
] } ] }
```

```rust
// plugins(): Box::new(goat_input::InputActionsPlugin::new("input/controls.inputactions.json"))
let a = goat_input::actions(ctx).unwrap();
let move_dir = a.read_vec2("Player/Move");
if a.triggered("Jump") { /* ... */ }
```

- Đường dẫn control theo Unity, bố cục Nintendo: `buttonSouth` = B, `buttonEast` = A, `buttonNorth` = X, `buttonWest` = Y (hoặc ghi thẳng `a` `b` `x` `y`), `leftShoulder` / `rightShoulder` = L / R, `leftTrigger` / `rightTrigger` = ZL / ZR, `start`, `select`, `dpad`, `leftStick` (+ `/x` `/up`...), `<Touchscreen>/press`, `<Pointer>/position`.
- Loại action: `button`, `value`, `passthrough`. Interaction: `press`, `hold(duration=…)`, `tap(duration=…)`. Processor: `invert`, `invertVector2`, `scale`, `scaleVector2`, `normalizeVector2`, `clamp`, `axisDeadzone`, `stickDeadzone`.
- Nhiều binding cùng lúc: binding bị tác động mạnh nhất thắng. `set_map_enabled`, đổi phím lúc chạy bằng `override_binding` + lưu / nạp `overrides_json`.
- Action được cập nhật ở đầu frame (`Plugin::early_update`), nên behaviour, fixed update và plugin cùng thấy một trạng thái.

## Tween, coroutine, lưu dữ liệu

**Tween** (`goat-tween`, giống DOTween): di chuyển / co giãn / xoay / punch / shake / đi theo đường cong, ease (31 kiểu của DOTween), lặp (Restart / Yoyo / Incremental), delay, relative, `From`, sequence (append / join / insert / khoảng nghỉ / callback). Vật bị xoá thì tween của nó tự huỷ.

```rust
tweens(ctx).add(Tween::move_to(coin, vec3(0.0, 2.0, 0.0), 0.6).ease(Ease::OutBack).loops(-1, LoopType::Yoyo));
tweens(ctx).add(Sequence::new().append(Tween::scale_to(e, Vec3::splat(1.3), 0.15)).append(Tween::scale_to(e, Vec3::ONE, 0.3).ease(Ease::OutBounce)).build());
```

Không cần code thì dùng component `TweenAnimation` trong scene (giống DOTweenAnimation):

```json
"TweenAnimation": { "animation": "Move", "end": [0, 0.4, 0], "relative": true, "duration": 1.2, "ease": "InOutSine", "loops": -1, "loopType": "Yoyo" }
```

Trong script viết như DOTween: `transform.doMove(v, 1).setEase(Ease.OutBack).onComplete(() => ...)`, `DOTween.sequence()`, `DOTween.to(() => x, v => x = v, 10, 1)`.

**Coroutine** như Unity (chạy sau `Update`), dừng khi vật sở hữu bị xoá:

```ts
start() { this.startCoroutine(this.blink()); }
*blink() { yield new WaitForSeconds(0.5); yield null; yield new WaitUntil(() => this.ready); }
async load() { await Awaitable.waitForSecondsAsync(1); }   // Awaitable của Unity 2023
```

- C# viết y như Unity (`IEnumerator` + `yield return new WaitForSeconds(1f)`, `StartCoroutine(...)`, `async void` + `await Awaitable.WaitForSecondsAsync(1)`).
- Rust: `start_coroutine(ctx, Some(me), |ctx| Yield::Seconds(1.0))` hoặc `Steps::new().wait(0.5).then(|ctx| ...).repeat().start(ctx, owner)`.
- `Time.timeScale` (0 = dừng, 0.5 = chậm) và `Time.unscaledDeltaTime` có ở mọi ngôn ngữ.

**Lưu dữ liệu** (`goat-save`, `SavePlugin::new("TenGame")`):
- `PlayerPrefs.setInt / getInt / setFloat / setString...`, tự ghi file khi có thay đổi (tối đa 1 lần mỗi giây) và khi thoát game.
- Save slot lưu JSON tuỳ ý: `SaveData.write("slot1", {...})` / `read` / `delete` / `slots()`.
- Ghi qua file tạm rồi mới đổi tên, nên mất điện giữa chừng không hỏng bản lưu cũ.
- Nơi lưu: Windows `%APPDATA%/TenGame/`, Linux `~/.local/share/TenGame/`, 3DS `sdmc:/3ds/TenGame/save/`.

## Script: Rust, TypeScript, C#, C / C++

Script gắn vào entity bằng component `Script` (tương đương MonoBehaviour). Mọi ngôn ngữ đi qua cùng một API nên chạy giống nhau, và chạy được trên cả desktop, editor lẫn 3DS.

```json
"Script": [ { "src": "scripts/Hover.ts", "props": { "height": 0.3 } }, { "src": "scripts/Pulse.cs" }, { "src": "native:Wobble" } ]
```

`props` là giá trị khởi tạo của field (như field serialize của Unity). Inspector hiện mỗi script như một component của Unity ("Hover (Script)": bật / tắt, field Script, rồi mọi field của class theo thứ tự khai báo với giá trị mặc định của class); field `_tên` là private, không hiện. Thứ tự gọi: `start` trước `update` đầu tiên, `update` rồi `lateUpdate` mỗi frame (sau behaviour Rust), `fixedUpdate` theo nhịp cố định, `onDestroy` khi entity hoặc script bị xoá. Script ném lỗi thì được ghi ra Console và tắt, các script khác vẫn chạy.

**TypeScript / JavaScript** (`goat-script-js`) — ES module, API import từ `"goat"`:

```ts
import { Behaviour, Input, Time, Vector3, GameObject } from "goat";
import { clamp } from "./util";              // script import lẫn nhau bằng đường dẫn tương đối

export default class Hover extends Behaviour {
  height = 0.25;                             // field → nhận giá trị từ "props"
  update(): void {
    this.transform.rotate(0, 90 * Time.deltaTime, 0);
    if (Input.getButtonDown("A")) GameObject.instantiate("prefabs/coin.prefab.json");
    if (Input.action("Player/Jump").triggered) this.gameObject.call("Animator.setTrigger", "jump");
    const rb = this.getComponent("Rigidbody2D");          // component bất kỳ dưới dạng JSON của scene
    if (rb) this.gameObject.patchComponent("Rigidbody2D", { velocity: [0, 5] });
  }
}
```

Callback sự kiện như Unity (gửi trước `update` của frame sau): `onCollisionEnter` / `Stay` / `Exit` (nhận `Collision`: `gameObject`, `contacts`, `relativeVelocity`), `onTriggerEnter` / `Stay` / `Exit` (nhận GameObject kia) và bản `2D` (`onCollisionEnter2D`...); con trỏ trên collider: `onMouseEnter` / `Over` / `Exit` / `Down` / `Drag` / `Up` / `UpAsButton` (luôn có), `onPointerEnter` / `Exit` / `Down` / `Up` / `Click` (nhận `PointerEventData`) khi camera dưới con trỏ có component **Physics Raycaster** / **Physics 2D Raycaster** (Add Component ▸ Event). Con trỏ = màn hình cảm ứng dưới của 3DS / chuột. Từ Rust: `goat_script::send_message(resources, entity, "onX", arg)`; tia từ màn hình: `goat_runtime::screen_point_to_ray` (Camera.ScreenPointToRay).

API:
- Cơ bản: `Vector2`, `Vector3`, `Quaternion`, `Transform` (position, eulerAngles, rotation, localScale, translate, rotate, lookAt, parent, `do*` tween), `GameObject` (find, create, instantiate, destroy, name, get / set / patch / removeComponent, call), `Behaviour` / `MonoBehaviour` (+ startCoroutine).
- Thời gian, input, log: `Time` (+ timeScale, unscaled), `Input` (getButton / Down / Up, stick, pointer, action), `Debug` (và `console`), `Mathf`, `Application.quit`.
- Coroutine, tween: các lệnh `WaitFor*` / `WaitUntil` / `WaitWhile`, `Awaitable`, `DOTween` / `Ease` / `LoopType`.
- Physics: `Physics`, `Physics2D` (raycast, overlap, collisions, addForce, moveCharacter).
- Audio: `Audio`, `AudioSource`, `AudioMixer`.
- Animation: `Animator` (tham số, play, events), `SpriteAnimator`.
- UI, lưu dữ liệu: `UI` (onClick, onValueChanged, onSubmit, value, setText, select...), `PlayerPrefs`, `SaveData`.
Engine JS là Boa (viết bằng Rust thuần, cùng một bản chạy mọi nơi). TypeScript được biên dịch bằng oxc lúc nạp (desktop, editor); `goat build` biên dịch sẵn ra `.js` cho console.

**C#** (`tools/goat-cs`, Roslyn) — viết như Unity, được kiểm tra kiểu theo API `GoatEngine` rồi dịch sang ES module `X.cs.js` chạy trên host JS:

```csharp
using GoatEngine;

public class Pulse : MonoBehaviour
{
    public float amount = 0.08f;
    Vector3 baseScale;
    float t;
    void Start() { baseScale = transform.localScale; }
    void Update()
    {
        t += Time.deltaTime;
        transform.localScale = baseScale * (1f + Mathf.Sin(t * 2f * Mathf.PI) * amount);
        if (Input.GetButtonDown("X")) Debug.Log($"pop at {t:F2}");
    }
}
```

- Tên Unity (`Start`, `Update`, `Debug.Log`, `GameObject.Find`, `Mathf.Sin`...); toán tử vector, `List<T>`, `Dictionary`, enum, property, string interpolation, lambda, `foreach`, `switch`, `try` đều được dịch. `GetComponent("Rigidbody2D")` trả về `dynamic`.
- Cần .NET SDK. Editor tự dịch khi file `.cs` đổi (lỗi hiện ở Console); `goat build` dịch trước khi đóng gói. Dịch tay: `dotnet tools/goat-cs/bin/goat-cs.dll <thư mục assets>` (build tool: `cd tools/goat-cs && dotnet build -c Release -o bin`). `goat-cs --api <thư mục>` xuất file API `GoatEngine.cs` cho IDE.

**C / C++** (`goat-script-native`, header `crates/goat-script-native/include/goat.h`) — lớp behaviour là một bảng callback, link tĩnh vào game (3DS không nạp thư viện động được):

```c
static void* create(GoatApi* g, GoatEntity self, const char* props_json) { return calloc(1, sizeof(State)); }
static void update(void* s, GoatApi* g, GoatEntity self) {
    GoatVec3 p = goat_get_position(g, self);
    p.y += goat_delta_time(g);
    goat_set_position(g, self, p);
    if (goat_button(g, "A") & GOAT_PRESSED) goat_spawn(g, "Marker");
}
static const GoatBehaviourClass WOBBLE = { "Wobble", create, NULL, update, NULL, NULL, NULL, free };
void goat_game_init(void) { goat_register_behaviour(&WOBBLE); }   // scene: "native:Wobble"
```

C++ bọc bảng này bằng một template class (xem `crates/goat-script-native/examples/orbit.cpp`).

**ScriptableObject** — asset dữ liệu dùng chung như Unity (TypeScript / JavaScript / C#):

```ts
import { ScriptableObject } from "goat";
export default class EnemyData extends ScriptableObject {
  maxHealth = 10;
  moveSpeed = 2.5;
  loot = null;                 // tham chiếu tới một ScriptableObject khác
  onEnable() {}                // gọi một lần khi asset được nạp
}
```

- Asset là file `.asset.json`: `{ "version": 1, "script": "scripts/EnemyData.ts", "data": { "maxHealth": 30 } }`. Tạo trong Project ▸ Create ▸ **ScriptableObject** ▸ *EnemyData* (menu liệt kê mọi script kế thừa `ScriptableObject`); Create ▸ Scripting có mẫu script Behaviour / ScriptableObject cho TS và C#.
- Inspector của asset hiện field của class theo thứ tự khai báo (giá trị mặc định lấy từ class, giá trị đã lưu đè lên), sửa là ghi file ngay.
- Field của script (Behaviour hoặc ScriptableObject) đặt bằng một `.asset.json` thì lúc chạy nhận **object** đó; mọi chỗ cùng tham chiếu một asset dùng chung một instance (asset tham chiếu vòng nhau cũng được). `ScriptableObject.load(key)` / `Resources.load(key)`, `ScriptableObject.createInstance(EnemyData)`, `ScriptableObject.instantiate(asset)` (bản sao "(Clone)"); `name`, `assetPath`. Thay đổi lúc Play không ghi vào asset (như bản build của Unity).
- C#: `public class EnemyData : ScriptableObject { public int maxHealth = 10; }`, `ScriptableObject.CreateInstance<EnemyData>()`, `Resources.Load<EnemyData>(key)`.

**Thêm lệnh cho script** (subsystem / game): `goat_script::extensions(ctx).add("Animator.setTrigger", f)` — mọi ngôn ngữ gọi được (`gameObject.call(...)`, `goat_call(...)`). `goat-animation` (feature `script`) đăng ký sẵn `Animator.setFloat / setInteger / setBool / setTrigger / resetTrigger / getFloat / getBool / currentState / play / crossFade`.

## Editor

Editor dùng Tauri 2 + React; engine chạy bên trong editor dưới dạng WebAssembly và render bằng WebGPU.

Giao diện và hành vi theo editor PiaEngine (skin "Unity Dark", icon lucide, font Inter):
- **Khung**: menu chính (bên phải: tên project), thanh công cụ chính (Hand / Move / Rotate / Scale, Pivot / Center, Global / Local, Play / Pause / Step ở giữa, nút Animator / Profiler / Build), dock panel kéo thả được (mặc định: Hierarchy | Scene + Game | Inspector, dưới là Project + Console), thanh trạng thái (thông báo Console gần nhất, Edit Mode / Playing / Paused). Ctrl+1 Scene, Ctrl+2 Game, Ctrl+3 Inspector, Ctrl+4 Hierarchy, Ctrl+5 Project, Ctrl+7 Profiler, Ctrl+Shift+C Console.
- **Dock như PiaEngine**: kéo tab sang nhóm khác, cạnh của một nhóm hoặc cạnh của cả dock để đổi chỗ (kéo bằng pointer: drag-and-drop HTML5 tắt trong webview vì kéo thả file của hệ điều hành đang bật). Chuột phải trên tab: **Pop Out to New Window** (panel chạy trong cửa sổ riêng, vẫn là cùng một editor; Scene view pop out vẫn render), **Float** (cửa sổ nổi trong editor, kéo thanh tiêu đề; Shift+kéo để gắn lại), **Dock** / **Dock to Main Window**, Maximize / Restore, Close Tab. Bố cục được nhớ.
- **Scene view** và **Game view** là hai tab riêng (dùng chung một canvas WebGPU: tab nào đang hiện thì giữ canvas). Scene view có thanh **2D** (camera trực giao nhìn theo −Z, chỉ kéo / zoom), **Grid**, **Gizmos**, hộp công cụ dọc ở góc, con quay trục góc phải (click một trục để nhìn theo trục đó, nhãn Persp / Iso để đổi phối cảnh / trực giao). Khi Play, Scene view vẫn xem được game đang chạy qua camera editor.
- **Game view**: vẽ bằng camera của scene ngay cả khi chưa Play (như Unity); menu kích thước (3DS 2 màn hình, Free Aspect, tỉ lệ, độ phân giải), Scale, Stats. Play tự chuyển sang Game view, Stop quay về Scene view.
- **Play mode**: Hierarchy và Inspector hiện thế giới đang chạy (cập nhật liên tục); sửa giá trị lúc Play có hiệu lực ngay và mất khi Stop, không ghi Undo — như Unity.
- **Hierarchy**: nút ＋▾ (menu GameObject), ô tìm kiếm (`t:Camera`, `t:Light`, `t:Mesh`... lọc theo loại), dòng scene, icon theo component chính (camera, đèn, particle, audio, mesh), menu chuột phải như PiaEngine (Cut / Copy / Paste / Paste As Child, Rename / Duplicate / Delete, Select Children / Frame Selected, Create Empty, 3D Object ▸ 2D Object ▸ Effects ▸ Light ▸ Audio ▸ UI ▸ Camera).
- **Project**: nút ＋▾ Create, ô tìm kiếm, cây thư mục, breadcrumb, lưới icon theo loại asset, thanh trượt cỡ icon; menu chuột phải: Create ▸, Open, Show in Explorer, Rename / Duplicate / Delete, Cut / Copy / Paste, Copy Path, Select All, Refresh.
- **Inspector**: đầu trang có icon object, checkbox Active, tên, Tag; mỗi component có icon, nút gập và menu ⋮. Chọn nhiều object (Ctrl / Shift + click): Inspector sửa tất cả cùng lúc như Unity — Transform hiện "—" ở trục có giá trị khác nhau, chỉ hiện component mà mọi object đều có, sửa một trường thì trường đó được đặt cho mọi object (các trường khác giữ nguyên), Add / Remove Component, Active, Tag, tên áp cho tất cả.
- **Prefab**: Inspector của object thuộc một instance prefab có thanh Prefab: *Open* (vào Prefab Mode), *Select* (chọn gốc instance), *Overrides ▾* liệt kê object / component đã sửa so với prefab, *Revert All* (dựng lại instance từ prefab) và *Apply All* (ghi các thay đổi vào file prefab, mọi instance cập nhật theo). Double-click file `.prefab.json` cũng vào **Prefab Mode**: Scene view sửa thẳng prefab, thanh breadcrumb `‹ Back · scene › prefab`, Back tự lưu prefab rồi quay lại scene trước.
- **Console**: Clear, Collapse, tìm kiếm, bật / tắt info / warning / error kèm số đếm, khung xem đầy đủ dòng đang chọn.
- Các lựa chọn (tool, Pivot / Local, Grid, Gizmos, kích thước Game view, Stats) được nhớ giữa các lần mở.
- **Menu chính** như Unity: File (New / Open / Save Scene), Edit (Undo / Redo kèm tên thao tác, Cut / Copy / Paste / Duplicate / Delete, Select All, Frame Selected, Play), Assets (Create, Show in Explorer), GameObject (Create Empty, 3D Object, 2D Object, Effects, Light, Audio, UI, Camera), Component (mọi component theo nhóm), Window (mở lại panel, Reset Layout).
- **Undo / Redo** (Ctrl+Z / Ctrl+Y) cho mọi thay đổi trong scene: tạo, xóa, đổi tên, đổi cha, gizmo, sửa component, thêm / bỏ component, dán. Kéo một số hoặc gõ liên tục vào cùng một trường được gộp thành một bước Undo.
- **Phím tắt**: Ctrl+S lưu, Ctrl+P Play, Ctrl+D nhân bản (tên `Cube (1)`), Ctrl+C / X / V, Ctrl+A, Delete, F2 đổi tên, F căn khung nhìn, Ctrl+Shift+N tạo GameObject rỗng, Alt+Shift+N tạo con.
- **Dock panel**: kéo tab để sắp xếp, chia đôi, ghép nhóm. Layout được nhớ lại; nút *Reset Layout* đưa về mặc định (Hierarchy | Scene/Game | Inspector, bên dưới là Project + Console).
- **Project (Asset Panel)**: dùng như một trình quản lý file.
  - Cây thư mục, breadcrumb, lưới có thumbnail, tìm kiếm.
  - Chọn nhiều (Ctrl/Shift/kéo khung), F2 đổi tên, Delete xóa vào Thùng rác, Ctrl+D/C/X/V, menu chuột phải.
  - Kéo thả qua drag gốc của hệ điều hành: vào thư mục (giữ Ctrl để copy), sang cửa sổ editor khác (*New Window*), ra Explorer hoặc ứng dụng khác; kéo file từ ngoài vào để import.
  - Đổi tên hay di chuyển asset thì tham chiếu trong scene, prefab, material được tự sửa theo.
- **Scene view**: double-click file `.scene.json` để mở.
  - Chuột phải hoặc Alt + chuột trái: xoay; chuột giữa: kéo khung nhìn; cuộn: zoom; F: căn vào vật đang chọn.
  - Click để chọn, Ctrl / Shift + click để thêm hoặc bỏ khỏi vùng chọn; Hierarchy và Inspector đồng bộ theo.
- **Công cụ Scene view** (như Unity): Q (✋, kéo chuột trái để di chuyển khung nhìn), W / E / R / Y chọn Move / Rotate / Scale / Transform. Tay cầm vẽ giống PiaEngine: ở 3D như three.js TransformControls (mũi tên có đầu nón quay về phía camera, ô mặt phẳng để kéo trên 2 trục, tâm để kéo tự do trên mặt phẳng nhìn; nửa vòng xoay hướng về camera, vòng vàng ngoài để xoay quanh hướng nhìn, kéo bên trong vòng xám để xoay tự do; khối vuông ở đầu trục Scale và ở tâm để co giãn đều; Transform = mũi tên + vòng xoay), ở chế độ 2D như tay cầm 2D của Pia (mũi tên X / Y, ô XY, một vòng xoay, ô vuông Scale). Rê chuột lên tay cầm thì nó sáng vàng; khi xoay / co giãn có số đo bên cạnh con trỏ. Vật đang chọn có khung cam (3D: hộp bao theo thế giới, 2D: khung của sprite), camera và đèn có icon (click để chọn), ở 2D camera hiện khung vùng nhìn. Mỗi lần kéo là một bước Undo. Nút *Pivot / Center* (Z): gizmo đặt ở vật đang active hay ở tâm các vật đang chọn — kéo là di chuyển / xoay / co giãn **mọi vật đang chọn** (Center: xoay / co giãn quanh tâm chung; Pivot: mỗi vật quanh pivot của nó). Nút *Global / Local* (X): trục theo thế giới hay theo vật (Scale luôn theo vật). Giữ **Ctrl** khi kéo để snap: 0.25 đơn vị, 15°, 0.1. Lưới trên mặt đất.
- **Hierarchy**: click / Ctrl+click / Shift+click để chọn nhiều, phím mũi tên để di chuyển và gập / mở, Alt+click mũi tên gập cả nhánh, ô tìm kiếm. Kéo một hoặc nhiều dòng thả lên dòng khác để đổi cha, thả vào chỗ trống để bỏ cha (vị trí trong thế giới được giữ nguyên). Chuột phải: Copy / Cut / Paste / Paste As Child, Rename, Duplicate, Delete, Select Children, Frame Selected và menu tạo GameObject. Instance prefab có chữ màu xanh; con sinh ra từ prefab / model in nghiêng.
- **Inspector**: tên GameObject, Transform, rồi từng component với tên kiểu Unity (`Mesh Renderer`, `Rigidbody`…), gập / mở được, nút ⋮ (hoặc chuột phải tiêu đề): Reset, Remove Component, Copy Component, Paste Component Values.
  - Trường được vẽ theo metadata của component: enum là danh sách chọn; kiểu union (Camera `projection`, Collider `shape`, Joint `kind`) là danh sách chọn kèm các trường con; asset (texture, material, mesh, font, audio…) là ô nhận asset thả từ Project hoặc chọn bằng nút ⊙; màu có thanh alpha; layer, culling mask; giá trị có giới hạn có thanh trượt; tooltip khi rê chuột vào tên trường. Ô số kéo nhãn để chỉnh và nhận biểu thức (`2*3`).
  - *Add Component*: popup có ô tìm kiếm và danh mục giống Unity (Mesh, Rendering, Effects, Physics, Physics 2D, Audio, UI, Layout, Event, Miscellaneous, Scripts). Component bắt buộc đi kèm được thêm tự động (ví dụ UI Button thêm Rect Transform và Image).
  - Game / package mô tả component của mình bằng `goat_scene::registry(ctx).describe("Name", ComponentInfo::new("Menu/Path").asset("texture", "texture").options(...))`.
- **Console**: Clear, Collapse (gộp dòng trùng), bật / tắt từng mức info / warn / error kèm số đếm, tìm kiếm, double-click để copy một dòng.
- Kéo model (.glb) hoặc prefab từ Project vào Scene view / Hierarchy để thêm vào scene.
- **Animator**: double-click file `.controller.json` để mở. Chuột phải nền: *Create State*; chuột phải state: *Make Transition* / *Set as Layer Default State* / *Delete*; click mũi tên để sửa transition; Alt + kéo hoặc chuột giữa để kéo khung nhìn; Ctrl+S lưu.
- **Script**: file `.cs` được tự dịch khi lưu, lỗi biên dịch và lỗi script lúc Play hiện ở Console.
- **Gizmo của component** (object đang chọn): khung collider 3D / 2D và CharacterController màu xanh lá, frustum của camera, tia / tầm / hình nón của đèn, tầm min / max của AudioSource 3D, hình trụ của NavMeshAgent, khối NavMeshObstacle.
- **Pause / Step** (❚❚ / ▶❚ cạnh nút Play, Ctrl+Shift+P / Ctrl+Alt+P): dừng game giữa chừng (Game view vẫn hiện khung hình), chạy từng frame một.
- **Profiler** (Window ▸ Profiler): biểu đồ CPU 300 frame gần nhất (ms của từng plugin, behaviour, transform, culling, render; vạch 60 / 30 FPS), click một cột để xem frame đó, bảng ms / %, số draw call và tam giác. Đo cả Play mode lẫn Edit mode.
- **Build Settings** (File ▸ Build Settings…, Ctrl+Shift+B): danh sách *Scenes In Build* (kéo để đổi thứ tự, bỏ tick để tạm loại, Delete để xóa, thả scene từ Project vào, nút *Add Open Scenes*; scene đầu tiên được bật là scene khởi động), chọn nền tảng Desktop / Nintendo 3DS, *Development Build*, nút *Build* chạy `goat build` trên project đang mở và in kết quả ra Console.
- **Project Settings** (Edit ▸ Project Settings…): Player (tên, phiên bản), Time (Fixed Timestep, Maximum Allowed Timestep), Physics / Physics 2D (gravity), Tags and Layers (tag thêm ở đây hiện trong ô Tag của Inspector). Tất cả lưu trong `assets/project.json`, áp dụng khi Play và trong bản build.
- **Play (▶)**: chạy game thật (behaviour, script, physics, animation, sprite, text) trong Game view. Menu kích thước trên thanh Scene / Game: *Nintendo 3DS* (2 màn hình 400×240 + 320×240, chọn trước khi Play), *Free Aspect*, tỉ lệ 16:9 / 16:10 / 4:3 / 9:16, độ phân giải cố định 1920×1080 / 1280×720 / 800×480 (vẽ đúng số pixel rồi thu nhỏ cho vừa); lựa chọn được nhớ lại. Play hoặc mở scene sẽ đưa tab Scene / Game lên trước. Phím điều khiển giống bản desktop. Esc hoặc ■ để dừng; scene đang edit không bị thay đổi.

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
