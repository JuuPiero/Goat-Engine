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
- ⬜ ETC1 / ETC1A4 (nén ở bước import trên PC — giai đoạn 2)
- ✅ 8 đèn mỗi view: directional / point (LUT suy giảm theo khoảng cách) / spot (LUT hình nón), chọn đèn gần camera nhất
- ✅ Fog tuyến tính (khối fog của PICA); 3DS dùng **w-buffer** → depth tuyến tính, chính xác đều cả gần lẫn xa
- ✅ Viewport một phần trên 3DS (camera picture-in-picture, clear từng phần)
- ✅ Frustum culling (bounding sphere của mesh, ở runtime cho mọi backend)
- ⬜ Ambient bán cầu, emissive map, cubemap phản chiếu
- ⬜ Batching / instancing, LOD

**Đẹp mà vẫn nhanh trên 3DS** (theo yêu cầu, làm dần):
- ✅ SSAA 2×2 khi tắt 3D (render 800×480, thu bằng display transfer — không tốn shader)
- ⬜ Khử răng cưa khi bật 3D: MSAA-lite bằng display transfer 2×1 mỗi mắt nếu đủ VRAM, hoặc FXAA-lite trên màn dưới
- ⬜ Alpha-to-coverage giả cho lá cây / rào (alpha test + dither)
- ⬜ Lightmap / vertex lighting nướng sẵn (bake trên PC) + fragment lighting cho vật động
- ⬜ Bóng: blob shadow (quad + texture), shadow map bằng tính năng shadow của PICA cho 1 đèn
- ⬜ Viền nhân vật (toon outline bằng back-face extrude), toon shading bằng LUT D0/D1
- ⬜ Rim light / fresnel bằng LUT fresnel của PICA, phản chiếu cubemap
- ⬜ Procedural texture (proctex) cho nước / lửa, fog gas cho khói
- ⬜ Shadow: blob / projected trên 3DS, shadow map trên desktop
- ⬜ Post-process trên desktop; trên 3DS chỉ màn hình phẳng (pha màu bằng texenv)
- ✅ Sprite / 2D (crate `goat-sprites`): Sprite + sprite sheet, SpriteRenderer (tint, flip, layer, sorting order), gom lô theo texture → 1 draw call mỗi lô
- ✅ Plugin (cách subsystem gắn vào frame: build / fixed_update / update / late_update), fixed timestep, `Behaviour::fixed_update` / `late_update`
- ✅ Layer + culling mask, sorting order, camera clear "depth only", mesh động (`update_mesh`, double-buffer trên 3DS), fog tắt được theo material
- ✅ Text (`goat-text`): font TTF/OTF vẽ lúc chạy vào atlas động (mọi ngôn ngữ, có tiếng Việt), căn lề, xuống dòng, kerning, chữ trong thế giới 3D và trên HUD
- ⬜ Text SDF (viền, bóng chữ), rich text

## Giai đoạn 2 — Tài nguyên & dữ liệu 🔨
- ✅ `goat-assets`: thư mục `assets/` của game (romfs trong .3dsx trên 3DS, cạnh file exe trên desktop), asset key theo đường dẫn, `AssetServer` cache + tra ngược handle → key
- ✅ PNG (mọi kiểu màu) + file import `<ảnh>.import.json` (filter, wrap, mipmap, định dạng, max_size); tự đưa về lũy thừa 2 khi GPU yêu cầu
- ✅ glTF 2.0 / GLB: mesh (tự tách khi > 65 535 đỉnh, tự tính normal), material PBR → Blinn-Phong, ảnh PNG nhúng, cây node; `spawn_model`
- ✅ Material file `*.mat.json`
- ✅ `goat-scene`: scene / prefab JSON, registry component + behaviour (serde), model / prefab instance, lưu ra file (round-trip có test)
- ⬜ Animation / skin của glTF (giai đoạn 3 — animation), JPEG, KTX / ETC1 nén sẵn
- ⬜ GUID + `.meta` (đổi tên file không vỡ tham chiếu), override sâu trong prefab, prefab variant
- ⬜ Bản build: chuyển JSON → nhị phân, nén texture trước cho từng nền tảng
- ⬜ Hot reload asset trên desktop
- ⬜ ScriptableObject-style data asset

## Giai đoạn 3 — Gameplay systems (theo PiaEngine)
- ✅ Physics 3D (`goat-physics`, Rapier): RigidBody dynamic / kinematic / static, Collider box / sphere / capsule, trigger, sự kiện va chạm, raycast, impulse; chạy được trên 3DS
- ⬜ Physics 2D (Rapier2D), joint, character controller, mesh collider, layer va chạm
- ✅ Audio (`goat-audio`): WAV / OGG, mixer phần mềm (volume, pan, pitch, loop, âm 3D), AudioSource / AudioListener, one-shot; ra loa qua cpal (desktop) / DSP ndsp (3DS); `GOAT_AUDIO_DUMP` ghi ra WAV để kiểm chứng
- ⬜ Audio: stream nhạc dài (không giải nén hết), mixer group / hiệu ứng, scene codec cho AudioSource
- ✅ Animation (`goat-animation`): Animator phát clip glTF (nhiều clip theo trọng số, crossfade, loop, tốc độ), skinning bằng CPU (4 xương/đỉnh, mesh riêng cho mỗi instance), codec scene
- ⬜ Animator Controller (state machine, blend tree, layer mask), animation event, sprite animation clip, GPU skinning trên desktop
- ✅ Particles (`goat-particles`): rate + burst, shape point / sphere / cone / box, khoảng min..max cho tuổi thọ / tốc độ / kích thước / góc, màu + kích thước theo tuổi, trọng lực, lực cản, không gian world/local, alpha/additive, billboard gom lô
- ⬜ Particles: curve / gradient đầy đủ, sub-emitter, va chạm, texture sheet animation, trail; exporter Unity (như PiaEngine)
- ✅ UI (`goat-ui`): Canvas theo màn hình, RectTransform (anchor / pivot / offset như Unity), Image, Text, Button, Toggle, Slider, cảm ứng / chuột; vẽ một lượt gom lô mỗi canvas
- ⬜ UI: layout group, scroll view, input field, dropdown, 9-slice, điều hướng bằng D-pad, codec scene
- ⬜ Tilemap, Spline, Tween (DOTween-like), Cinemachine-like camera, Input System (action map, rebinding)
- ⬜ Coroutine / async (Awaitable), Job System (rayon trên desktop, 2–4 nhân trên New 3DS)
- ⬜ Netcode (UDS/local wireless trên 3DS, WebSocket/UDP trên desktop)
- ⬜ Save data (archive SD / extdata trên 3DS)

## Giai đoạn 4 — Script đa ngôn ngữ
Mọi ngôn ngữ đều đi qua cùng một trait `Behaviour`; engine không cần biết script viết bằng gì.
- ⬜ **C ABI** (`goat.h`, sinh bằng cbindgen): C và C++ gọi engine qua handle. Trên mọi nền tảng, script C/C++ được link tĩnh.
- ⬜ **TypeScript**: biên dịch TS → JS (esbuild), chạy trên QuickJS (rquickjs) ở mọi nền tảng kể cả 3DS. API giống PiaEngine (`MonoBehaviour`, `getComponent`...) để tái dùng code.
- ⬜ **C#**: trên desktop, host CoreCLR qua hostfxr (hot reload). Trên console, dùng NativeAOT hoặc transpile sang TS/C (tái dùng hướng tiếp cận của com.pia.csharp).
- ⬜ **Rust**: viết `Behaviour` trực tiếp (nhanh nhất)
- ⬜ Hot reload script trên desktop

## Giai đoạn 5 — Editor
- ⬜ Editor desktop: Hierarchy, Scene view (gizmo), Game view (có layout 3DS), Inspector, Project, Console, Play mode, Undo/Redo
- ⬜ Hướng đi đề xuất: editor native viết bằng egui trên chính wgpu backend (một process, đọc/ghi scene trực tiếp). Phương án khác: dùng lại UI web của PiaEngine qua IPC.
- ⬜ Chạy thử trên 3DS từ editor (`3dslink` + log qua mạng), profiler

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
