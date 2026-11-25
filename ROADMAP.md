# Lộ trình GoatEngine

Mục tiêu bản đầu là **ngang tính năng với PiaEngine**, nhưng chạy native và lấy 3DS làm nền tảng chuẩn. Mọi tính năng phải chạy được trên 3DS: 128 MB RAM (khoảng 64 MB cho app), CPU ARM11 268 MHz (New 3DS: 804 MHz, 4 nhân) và GPU PICA200 có pipeline cố định một phần. Tính năng nào nặng hơn sức 3DS thì vẫn có, nhưng gắn cờ theo nền tảng (ví dụ shadow map trên desktop, baked lighting trên 3DS).

Ký hiệu: ✅ xong · 🔨 đang làm · ⬜ chưa làm

## Giai đoạn 0 — Nền móng ✅
- ✅ Workspace Rust chia lớp: core / gfx (trung lập) / runtime / backend / platform / player / tools
- ✅ ECS (hecs), Transform phân cấp, Camera, MeshRenderer, DirectionalLight, Input trừu tượng, `Behaviour`
- ✅ CLI `goat` với `Builder` cho mỗi nền tảng (`build` / `run` / `doctor`), cấu hình máy trong `goat.local.toml`

## Giai đoạn 1 — Renderer 3DS + desktop 🔨
- ✅ citro3d: fragment lighting per-pixel, LUT specular, SSAA 2×2, stereo 3D theo thanh trượt, texture tile Morton, sắp xếp opaque/blend
- ✅ wgpu (Vulkan): Blinn-Phong, nhiều viewport, layout xem trước 2 màn hình 3DS, chụp màn hình, color space linear
- ✅ Mipmap (cả hai backend), định dạng 16-bit cho 3DS: RGB565 / RGBA4 / RGBA5551
- ✅ ETC1 / ETC1A4 trên 3DS (`"format": "etc1" | "etc1a4"` trong file import: 4 / 8 bit mỗi pixel; bộ mã hóa thử cả hai cách chia khối, chế độ individual / differential và 8 bảng cường độ; desktop giữ RGBA8). Hiện nén lúc nạp, bản build sẽ nén sẵn.
- ✅ 8 đèn mỗi view: directional / point (LUT suy giảm theo khoảng cách) / spot (LUT hình nón), chọn đèn gần camera nhất
- ✅ Fog tuyến tính (khối fog của PICA); 3DS dùng **w-buffer** → depth tuyến tính, chính xác đều cả gần lẫn xa
- ✅ Viewport một phần trên 3DS (camera picture-in-picture, clear từng phần)
- ✅ Frustum culling (bounding sphere của mesh, ở runtime cho mọi backend)
- ✅ Ambient bán cầu (`RenderSettings.ambient_ground`; 3DS: ambient đất + một đèn từ trên xuống), emissive map, cubemap phản chiếu (`env_map` + `reflectivity`, `builtin:sky` / `.cubemap.json`; 3DS: shader thứ hai đưa vector phản chiếu vào texcoord0 cho unit cube)
- ✅ Static batching (`StaticBatching` trên root: gộp mesh cùng material thành một draw), LOD (`LodGroup` chọn mesh theo chiều cao trên màn hình, cull), profiler CPU theo plugin (`Profiler`: ms mỗi phần + số draw call). PICA không có instancing phần cứng.

**Đẹp mà vẫn nhanh trên 3DS** (theo yêu cầu, làm dần):
- ✅ SSAA 2×2 khi tắt 3D (render 800×480, thu bằng display transfer — không tốn shader)
- ✅ Khử răng cưa khi bật 3D: mỗi mắt render 480×400 (depth 16-bit) rồi display transfer thu 2×1; tất cả target cấp sẵn lúc khởi động (VRAM 6 MB vừa đủ)
- ✅ Alpha test cho lá cây / rào (`alpha_cutoff`), desktop thêm dither Bayer 4×4 (`alpha_dither`) như alpha-to-coverage giả
- ✅ Ánh sáng nướng vào vertex cho vật tĩnh (`StaticBatching { bake_lighting }`: ambient / bán cầu + mọi đèn, vẽ unlit) + fragment lighting cho vật động. Lightmap UV2 chưa có.
- ✅ Bóng blob (`BlobShadow`, goat-physics feature `render`: raycast xuống đất, theo độ dốc, nhỏ và nhạt dần theo độ cao)
- ⬜ Shadow map bằng tính năng shadow của PICA cho 1 đèn
- ✅ Toon: viền (`outline_width` / `outline_color`, vẽ lại mặt sau đẩy theo normal; 3DS: shader PICA thứ ba), cel shading (`toon_steps`; 3DS: specular0 × LUT D0 theo L·N)
- ✅ Rim light (`rim_color` / `rim_power`; 3DS: specular1 × LUT D1 theo N·V), phản chiếu cubemap
- ✅ Procedural texture (`procedural`: water / fire / noise; 3DS: đơn vị proctex của PICA, desktop: noise trong shader)
- ✅ Goat Shader: file `.shader` (ShaderLab + HLSL như Unity) dịch sang WGSL cho desktop / web, Fallback cho 3DS; Inspector material / shader, Create ▸ Shader, nạp lại khi sửa
- ✅ Goat Shader trên 3DS: hàm fragment → texture combiner (TEV) của PICA (kiểm bằng bộ mô phỏng TEV + Citra)
- ⬜ Goat Shader: keyword, nhiều Pass, alpha test (`clip`) và uv transform trên 3DS
- ⬜ Fog gas cho khói
- ✅ Shadow map trên desktop (đèn directional `shadows`, 2048², PCF 3×3, đi theo camera); 3DS dùng blob shadow
- ✅ Post-process (`RenderSettings.post`): desktop có exposure / contrast / saturation / tint / vignette / bloom; 3DS pha màu exposure × tint trong texenv stage 4
- ✅ Sprite / 2D (crate `goat-sprites`): Sprite + sprite sheet, SpriteRenderer (tint, flip, layer, sorting order), gom lô theo texture → 1 draw call mỗi lô
- ✅ Plugin (cách subsystem gắn vào frame: build / fixed_update / update / late_update), fixed timestep, `Behaviour::fixed_update` / `late_update`
- ✅ Layer + culling mask, sorting order, camera clear "depth only", mesh động (`update_mesh`, double-buffer trên 3DS), fog tắt được theo material
- ✅ Text (`goat-text`): font TTF/OTF vẽ lúc chạy vào atlas động (mọi ngôn ngữ, có tiếng Việt), căn lề, xuống dòng, kerning, chữ trong thế giới 3D và trên HUD
- ✅ Viền chữ, bóng chữ, rich text (`<color>`, `<size>`, `<b>`, `<i>`) cho Text và UiText, giống nhau trên mọi backend. Font SDF thật chưa có.

## Giai đoạn 2 — Tài nguyên & dữ liệu 🔨
- ✅ `goat-assets`: thư mục `assets/` của game (romfs trong .3dsx trên 3DS, cạnh file exe trên desktop), asset key theo đường dẫn, `AssetServer` cache + tra ngược handle → key
- ✅ PNG (mọi kiểu màu) + file import `<ảnh>.import.json` (filter, wrap, mipmap, định dạng, max_size); tự đưa về lũy thừa 2 khi GPU yêu cầu
- ✅ glTF 2.0 / GLB: mesh (tự tách khi > 65 535 đỉnh, tự tính normal), material PBR → Blinn-Phong, ảnh PNG nhúng, cây node; `spawn_model`
- ✅ Material file `*.mat.json`
- ✅ `goat-scene`: scene / prefab JSON, registry component + behaviour (serde), model / prefab instance, lưu ra file (round-trip có test)
- ✅ JPEG (zune-jpeg; cả ảnh JPEG trong glTF), KTX 1.1: ETC1 nén sẵn (`TextureData.compressed`: 3DS chỉ xếp lại khối, không nén lại trên máy), RGBA8 / RGB8; ví dụ `to_ktx`
- ✅ GUID + `.meta` (key `guid:...`; editor phát hiện file đổi tên / di chuyển bên ngoài và sửa tham chiếu), override sâu trong prefab (đường id xuyên prefab lồng nhau, JSON merge patch, editor tự ghi khi lưu, entity thêm vào trong prefab), prefab variant (`base`)
- ⬜ Bản build: chuyển JSON → nhị phân, nén texture trước cho từng nền tảng
- ⬜ Hot reload asset trên desktop
- ✅ ScriptableObject (`.asset.json`, TS / JS / C#, Inspector + Create menu, tham chiếu từ field script)

## Giai đoạn 3 — Gameplay systems (theo PiaEngine)
- ✅ Physics 3D (`goat-physics`, Rapier): RigidBody dynamic / kinematic / static, Collider box / sphere / capsule, trigger, sự kiện va chạm, raycast, impulse; chạy được trên 3DS
- ✅ Physics 2D (`goat-physics2d`, Rapier 2D — crate riêng, game không dùng thì không mang theo): Rigidbody2D / Collider2D (box, circle, capsule, polygon lồi) trên mặt phẳng XY, trigger, sự kiện, raycast, overlap point; scene `physics2d.scene.json`
- ✅ Joint (3D: fixed / hinge có giới hạn + motor / ball / slider / spring / rope; 2D: fixed / hinge / slider / spring / distance), CharacterController (trượt theo tường, leo bậc, dốc, grounded), mesh collider (trimesh + convex hull), edge collider 2D, layer va chạm (Layer Collision Matrix) + raycast / overlap theo layer mask, lệnh `Physics.*` / `Physics2D.*` cho script (GameObject đi qua lại được)
- ✅ Audio (`goat-audio`): WAV / OGG, mixer phần mềm (volume, pan, pitch, loop, âm 3D), AudioSource / AudioListener, one-shot; ra loa qua cpal (desktop) / DSP ndsp (3DS); `GOAT_AUDIO_DUMP` ghi ra WAV để kiểm chứng
- ✅ Audio nâng cao: stream Ogg dài (giải mã dần khi phát, tự bật khi file > 512 KB hoặc `load_type` trong `.import.json`), mixer group (`audio/mixer.json`: volume, mute, low-pass, lồng dưới Master), pause / thời gian phát, codec scene, lệnh `Audio.*` / `AudioSource.*` / `AudioMixer.*` cho script
- ✅ Animation (`goat-animation`): Animator phát clip glTF (nhiều clip theo trọng số, crossfade, loop, tốc độ), skinning bằng CPU (4 xương/đỉnh, mesh riêng cho mỗi instance), codec scene
- ✅ Animator Controller (`.controller.json`): tham số float / int / bool / trigger, state (clip hoặc blend tree 1D đồng bộ thời gian), transition có điều kiện / exit time / crossfade, Any State, layer override theo weight; cửa sổ Animator trong editor (đồ thị state, kéo node, Make Transition, Inspector state / transition)
- ✅ Avatar mask (`mask` theo tên node + con), layer additive, blend tree 2D (Freeform Cartesian, gradient band), animation event (`events` theo clip → `Animator.fired` / `Animator.of(go).onEvent`), sprite flipbook (`SpriteAnimator`: lưới hoặc rect, fps, loop, `next`)
- ⬜ GPU skinning trên desktop, xem state đang chạy trực tiếp trong cửa sổ Animator khi Play
- ✅ Particles (`goat-particles`): rate + burst, shape point / sphere / cone / box, khoảng min..max cho tuổi thọ / tốc độ / kích thước / góc, màu + kích thước theo tuổi, trọng lực, lực cản, không gian world/local, alpha/additive, billboard gom lô
- ✅ Particles nâng cao: gradient / curve theo tuổi (màu, kích thước, tốc độ), màu đầu random giữa hai màu, velocity over lifetime, texture sheet animation, va chạm (mặt phẳng + collider 3D với feature `physics`: nảy, mất tuổi thọ), sub-emitter (birth / death / collision), trail; exporter Unity → GoatEngine (`tools/unity-goat-exporter`, đã chạy thật bằng Unity 6 batch mode)
- ✅ UI (`goat-ui`): Canvas theo màn hình, RectTransform (anchor / pivot / offset như Unity), Image, Text, Button, Toggle, Slider, cảm ứng / chuột; vẽ một lượt gom lô mỗi canvas
- ✅ UI nâng cao: Image Sliced (9-slice) / Filled, layout group ngang / dọc / lưới + Layout Element + Content Size Fitter, mask cắt (RectMask2D), Scroll Rect (kéo, quán tính, bật lại ở mép), Input Field (gõ phím trên desktop, bàn phím hệ thống swkbd trên 3DS), Dropdown, điều hướng D-pad / analog (EventSystem), codec scene, lệnh `UI.*` cho script
- ✅ Input actions (`goat-input`, như Input System của Unity): `.inputactions.json` (action map, binding theo đường dẫn Unity `<Gamepad>/buttonSouth`...), composite 2DVector / 1DAxis, interaction press / hold / tap, processor (deadzone, invert, scale, normalize, clamp), bật/tắt map, rebinding + lưu override JSON; cập nhật đầu frame (`Plugin::early_update`)
- ⬜ Cửa sổ Input Actions trong editor, bàn phím / chuột như device riêng
- ✅ Tween (`goat-tween`, như DOTween): move / scale / rotate / punch / shake / path, 31 ease, loop Restart / Yoyo / Incremental, delay, relative, From, sequence; component `TweenAnimation`; API DOTween trong TS / C#
- ⬜ Tilemap, Spline, Cinemachine-like camera
- ✅ Coroutine (Rust: hàm bước / `Steps`; TS: generator; C#: `IEnumerator` + `yield return`), `Awaitable` / async-await, `Time.timeScale` + thời gian unscaled
- ⬜ Job System (rayon trên desktop, 2–4 nhân trên New 3DS)
- ⬜ Netcode (UDS/local wireless trên 3DS, WebSocket/UDP trên desktop)
- ✅ Lưu dữ liệu (`goat-save`): PlayerPrefs + save slot JSON, ghi an toàn (file tạm + đổi tên); AppData trên desktop, thẻ SD trên 3DS

## Giai đoạn 4 — Script đa ngôn ngữ ✅
Mọi ngôn ngữ đi qua cùng một lõi (`goat-script`: component `Script`, trait `ScriptHost`, một API `Api` dùng chung, `Extensions` để subsystem thêm lệnh như `Animator.setTrigger`), nên hành vi giống hệt nhau.
- ✅ **Rust**: `Behaviour` trực tiếp (nhanh nhất)
- ✅ **TypeScript / JavaScript** (`goat-script-js`): ES module `export default class X extends Behaviour`, import API từ `"goat"` và import lẫn nhau; engine JS Boa (Rust thuần → chạy y hệt trên desktop, editor wasm, 3DS); TS biên dịch bằng oxc (lúc nạp trên desktop / editor, lúc build cho console)
- ✅ **C#** (`tools/goat-cs`, Roslyn): kiểm tra kiểu theo API `GoatEngine` (tên Unity), dịch sang ES module `X.cs.js` chạy trên host JS; editor tự dịch khi file .cs đổi, lỗi hiện ở Console; `goat build` dịch trước khi đóng gói
- ✅ **C / C++** (`goat-script-native`, `include/goat.h`): lớp behaviour = bảng callback, đăng ký bằng `goat_register_behaviour`, link tĩnh; ví dụ C + C++ (template class) có test
- ⬜ QuickJS làm host JS nhanh hơn trên 3DS (cùng trait `ScriptHost`), hot reload script khi Play, debugger, field script hiện trong Inspector

## Giai đoạn 5 — Editor 🔨
- ✅ Tauri 2 + React + dockview; engine chạy dạng wasm (WebGPU / WebGL2) trong Scene / Game view
- ✅ Project panel như file manager (kéo thả native giữa 2 editor / ra Explorer / vào từ OS), Hierarchy (tạo, xoá, kéo để đổi cha), Inspector mọi component (JSON theo codec scene, Add / Remove Component), Console (kể cả log engine + lỗi script)
- ✅ Scene view: lưới, gizmo Move / Rotate / Scale (W / E / R), chọn bằng click, orbit / pan / zoom, F để frame, kéo model / prefab vào scene; Play / Stop; lưu scene
- ✅ Cửa sổ Animator (Animator Controller)
- ✅ Editor kiểu Unity / PiaEngine (đợt 1): menu chính, Undo / Redo (snapshot scene, gộp thao tác kéo), chọn nhiều, copy / paste / duplicate, menu GameObject, Hierarchy (chuột phải, F2, tìm kiếm, phím mũi tên), Inspector theo metadata component (`ComponentInfo`: menu, enum, variant, asset, màu, layer, mask, range), Add Component có tìm kiếm, Console (Collapse, lọc)
- Đuổi kịp PiaEngine (theo yêu cầu 2026-11-24), làm theo đợt:
  - ✅ Trail Renderer + Line Renderer (`goat-lines`), NavMesh: Surface / Agent / Obstacle / Modifier + truy vấn (`goat-navigation`, có nút Bake và lưới phủ trong editor, API script JS / C#)
  - ✅ GameObject active / inactive (cả cây con: vẽ, script, physics, UI, particle, audio, navigation) + tag, trong scene / prefab override / script / editor
  - ✅ Mesh Capsule / Cylinder / Quad; UI: Raw Image, Canvas Group, Aspect Ratio Fitter, Toggle Group, Scrollbar (nối ScrollRect), Rect Mask 2D, Shadow / Outline
  - ✅ Constant Force (3D / 2D), Area / Point / Buoyancy Effector 2D, Target Joint 2D
  - ✅ Sprite Mask (hình chữ nhật, Visible Inside / Outside)
  - ✅ Platform / Surface Effector 2D, Wheel / Friction / Relative Joint 2D
  - ✅ Prefab Mode (mở / Back tự lưu) + thanh Prefab trong Inspector: Overrides, Revert All, Apply All
  - ✅ Inspector sửa nhiều object cùng lúc (giá trị khác nhau hiện —)
  - ✅ Callback cho script: onCollision* / onTrigger* (3D + 2D), onMouse*, onPointer* (Physics Raycaster / Physics 2D Raycaster), Camera.ScreenPointToRay
  - ⬜ Đợt 2 — component lõi còn thiếu: Constant Force, joint 3D đủ loại (Character / Configurable) và joint / effector 2D, Sprite Mask, Light / Reflection Probe đơn giản, Animation (legacy), UI: Raw Image, Toggle Group, Scrollbar, Rect Mask 2D, Shadow / Outline, Grid Layout, Aspect Ratio Fitter, Canvas Group, raycaster; mesh dựng sẵn capsule / cylinder / quad
  - ✅ Pause / Step, cửa sổ Profiler (đồng hồ cho wasm, lịch sử frame)
  - ✅ Gizmo của component trong Scene view (collider, camera, đèn, audio, navigation)
  - ✅ Build Settings (danh sách scene, nền tảng, Build từ editor) + Project Settings (`assets/project.json`: Time, gravity, tag, layer) + `SceneManager.loadScene` (JS / C#)
  - ✅ Game view: menu kích thước (3DS, Free Aspect, tỉ lệ, độ phân giải cố định)
  - ✅ Scene view: tool Hand (Q), Pivot / Center, Local / Global, snap khi giữ Ctrl, kéo gizmo cho cả vùng chọn
  - ✅ Tay cầm như PiaEngine (three.js TransformControls ở 3D, tay cầm 2D của Pia ở 2D), Transform tool (Y), hover, khung chọn cam, icon camera / đèn
  - ✅ Giao diện editor như PiaEngine: menu / toolbar / dock / status bar, Scene và Game view riêng (2D, Grid, Gizmos, con quay trục, Persp / Iso, Scene view khi Play, Game view khi chưa Play), Hierarchy / Inspector của game đang chạy, menu Hierarchy / Project như Pia
  - ⬜ Đợt 3 — cửa sổ: Animation window, Package Manager, tool Rect, behaviour (script Rust) trong Inspector
- ⬜ Prefab mode, cửa sổ Input Actions, chạy thử trên 3DS từ editor (`3dslink` + log qua mạng), profiler

## Giai đoạn 6 — Thêm nền tảng (làm sau cùng, khi các thành phần engine đã xong)
Mỗi nền tảng = backend đồ họa + platform + `Builder` trong `tools/goat`.
- ⬜ **Web builder**: wasm32 + WebGPU/WebGL2 qua chính backend wgpu, `goat build web` → `out/web/` (index.html + .wasm + .js); sau đó Playable Ads (một file HTML)
- ⬜ Android / iOS (wgpu: Vulkan / Metal)
- ⬜ Nintendo Switch homebrew (devkitA64 + libnx, deko3d)
- ⬜ Wii / GameCube (devkitPPC + libogc, GX)
- ⬜ PS Vita (VitaSDK, vitaGL), New 3DS (tận dụng 804 MHz + nhân phụ)
- ⬜ Dedicated server (headless)

## Nguyên tắc
- Lõi (`goat-core`, `goat-gfx`, `goat-runtime`) không bao giờ chứa code riêng của một nền tảng.
- Không cấp phát trong vòng lặp frame ở đường nóng: dùng pool, buffer dùng lại, typed handle.
- Mỗi tính năng đều được kiểm chứng thật: 3DS trên emulator (và máy thật khi có thể), desktop bằng ảnh chụp tự động.
- ✅ Cửa sổ Animation (dopesheet, Preview, Record, Add Property) + component Animation + API script (2026-11-25)
