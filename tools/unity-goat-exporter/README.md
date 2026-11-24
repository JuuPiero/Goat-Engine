# unity-goat-exporter

Công cụ Editor của Unity: xuất `ParticleSystem` từ Unity sang prefab của GoatEngine (`.prefab.json`) kèm texture PNG.

## Cài vào project Unity

Chép thư mục `Editor/` vào `Assets/` của project Unity (Unity 2021+, đã thử với Unity 6000.0).

## Dùng

1. Chọn GameObject chứa các ParticleSystem (chính nó và các con).
2. Vào **Tools ▸ GoatEngine ▸ Export Particle Systems…** và chọn một thư mục trong `assets/` của game GoatEngine.
3. Kết quả là `<Tên>.prefab.json` (mỗi system thành một entity), texture nằm trong `particles/`.
4. Trong game: `goat_scene::instantiate(ctx, "fx/Firework.prefab.json", None)`, hoặc kéo prefab vào Scene trong editor.

## Những gì được chuyển

| Unity | GoatEngine (`ParticleSystem`) |
|---|---|
| Main: duration, loop, max particles, start lifetime / speed / size / rotation (hằng số hoặc hai hằng số), start color (một màu hoặc random giữa hai màu), gravity modifier, simulation space | `duration`, `looping`, `max_particles`, `lifetime` / `speed` / `size` / `rotation` (min..max), `start_colors`, `gravity`, `space` |
| Emission: rate over time, bursts (kể cả lặp) | `rate`, `bursts` |
| Shape: cone, sphere / hemisphere, box, circle | `shape` (hướng phát +Z của Unity được xoay về +Y) |
| Color over Lifetime (gradient) | `color_over_life` |
| Size over Lifetime (curve) | `size_curve` (8 điểm mẫu) |
| Velocity over Lifetime (hằng số) | `velocity_over_life` |
| Rotation over Lifetime (Z) | `spin` |
| Limit Velocity (dampen) | `drag` |
| Texture Sheet Animation (Grid) | `texture_sheet` |
| Collision: planes / world | `collision` (`world: true` cần feature `physics`) |
| Sub Emitters: birth / death / collision | `sub_emitters` (system con chỉ phát khi system cha gọi) |
| Trails | `trail` |
| Renderer: texture chính, additive / alpha, sorting order | `texture`, `blend`, `sorting_order` |

- Tọa độ: Unity dùng hệ tay trái, GoatEngine dùng hệ tay phải. Vì vậy vị trí và vector bị lật trục Z, phép xoay bị lật X / Y.
- Các module chưa có bên GoatEngine (noise, external forces, lights...) được bỏ qua.
- Curve dạng hai đường cong được lấy trung bình.

## Kiểm tra (batch mode)

```
Unity.exe -batchmode -quit -createProject <thư mục tạm>
chép Editor/GoatParticleExporter.cs và Tests~/MakeFx.cs vào <thư mục tạm>/Assets/Editor/
set GOAT_FX_OUT=<thư mục xuất>
Unity.exe -batchmode -quit -nographics -projectPath <thư mục tạm> -executeMethod MakeFx.Run
```

`MakeFx` tạo một pháo hoa: tên lửa hình nón, có gradient, size curve, texture sheet, trail, mặt phẳng va chạm và sub-emitter "Sparks" khi hạt chết. Bản xuất đã có sẵn ở `crates/goat-particles/tests/fixtures/`; test `cargo test -p goat-particles --features scene --test unity_export` chạy nó trong GoatEngine.
