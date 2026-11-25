//! The sample game. It only uses `goat-runtime`: the same code runs on desktop (Vulkan) and on the
//! Nintendo 3DS (PICA200), each player crate wraps it in its platform.

use goat_runtime::gfx::{BlendMode, Display, Fog, MaterialDesc, MeshData, Projection, TextureData, TextureFormat, Viewport};
use goat_runtime::math::{Quat, Vec3};
use goat_runtime::*;

mod hud;
mod ui_demo;

pub struct HelloGame;

/// The subsystems the game uses (also what the editor's Play mode runs with).
pub fn plugins() -> Vec<Box<dyn Plugin>> {
    vec![
        Box::new(goat_assets::AssetsPlugin::with_dev_root(concat!(env!("CARGO_MANIFEST_DIR"), "/assets"))),
        Box::new(goat_scene::ScenePlugin),
        Box::new(goat_assets::StaticBatchPlugin),
        Box::new(goat_input::InputActionsPlugin::new("input/controls.inputactions.json")),
        Box::new(goat_sprites::SpritePlugin::default()),
        Box::new(goat_text::TextPlugin::default()),
        Box::new(goat_physics::PhysicsPlugin),
        Box::new(goat_physics::BlobShadowPlugin::default()),
        Box::new(goat_physics2d::Physics2DPlugin),
        Box::new(goat_audio::AudioPlugin::default()),
        Box::new(goat_animation::AnimationPlugin::default()),
        Box::new(goat_particles::ParticlePlugin::default()),
        Box::new(goat_lines::LinePlugin::default()),
        Box::new(goat_navigation::NavigationPlugin),
        Box::new(goat_ui::UiPlugin::default()),
        Box::new(goat_tween::TweenPlugin),
        Box::new(goat_save::SavePlugin::new("GoatHello")),
        // scripts: TypeScript / JavaScript / C# (translated by goat-cs) and native C / C++
        Box::new(goat_script::ScriptPlugin::new().with_host(goat_script_js::JsHost::new()).with_host(goat_script_native::NativeHost::new())),
    ]
}

/// The game's behaviours that scenes / prefabs can hold.
pub fn register(reg: &mut goat_scene::SceneRegistry) {
    reg.behaviour::<Spinner>("Spinner").behaviour::<Bob>("Bob").behaviour::<FaceCamera>("FaceCamera");
}

/// Build with `GOAT_HELLO_FREEZE=1` to stop every animation (identical frames on every platform:
/// screenshot comparisons).
pub(crate) const FROZEN: bool = option_env!("GOAT_HELLO_FREEZE").is_some();
/// `GOAT_HELLO_LIGHT_TEST=1`: no sun, a pure red point light and a pure green spot (backend checks).
const LIGHT_TEST: bool = option_env!("GOAT_HELLO_LIGHT_TEST").is_some();

fn delta(ctx: &Context) -> f32 {
    if FROZEN { 0.0 } else { ctx.time.delta }
}

/// Turns its entity around an axis.
#[derive(serde::Serialize, serde::Deserialize)]
struct Spinner {
    axis: Vec3,
    speed: f32,
}

impl Behaviour for Spinner {
    fn update(&mut self, ctx: &mut Context, me: Entity) {
        let dt = delta(ctx);
        if let Some(mut t) = ctx.transform_mut(me) {
            t.rotation = (Quat::from_axis_angle(self.axis, self.speed * dt) * t.rotation).normalize();
        }
    }
}

/// Drops a bouncy ball every `interval` seconds (up to `max`), recycling the oldest.
struct BallSpawner {
    mesh: goat_runtime::gfx::MeshId,
    materials: Vec<goat_runtime::gfx::MaterialId>,
    interval: f32,
    timer: f32,
    max: usize,
    balls: Vec<Entity>,
}

impl Behaviour for BallSpawner {
    fn update(&mut self, ctx: &mut Context, _me: Entity) {
        // a bonk for balls hitting things (a few per frame at most), louder when faster
        let hits: Vec<Entity> = goat_physics::physics(ctx)
            .events
            .iter()
            .filter(|e| e.kind == goat_physics::CollisionKind::Enter)
            .flat_map(|e| [e.a, e.b])
            .filter(|e| self.balls.contains(e))
            .take(4)
            .collect();
        for b in hits {
            let (pos, speed) = (
                ctx.world.get::<&GlobalTransform>(b).map(|g| g.0.w_axis.truncate()).unwrap_or_default(),
                ctx.world.get::<&goat_physics::RigidBody>(b).map(|r| r.velocity.length()).unwrap_or(1.0),
            );
            if speed > 0.6 {
                let pitch = 0.8 + (b.id() % 5) as f32 * 0.1;
                goat_audio::play_at(ctx, "bonk", pos, (speed / 6.0).min(1.0), pitch);
            }
        }
        let (rain, dropping) = match ctx.resources.get_mut::<ui_demo::Controls>() {
            Some(c) => {
                let d = c.drop > 0;
                c.drop = c.drop.saturating_sub(1);
                (c.rain, d)
            }
            None => (1.0, false),
        };
        self.timer -= delta(ctx) * rain;
        if self.timer > 0.0 && !dropping {
            return;
        }
        if !dropping {
            self.timer = self.interval;
        }
        goat_audio::play_one_shot(ctx, "sfx/coin.wav", 0.35);
        let n = self.balls.len();
        let x = ((n as f32 * 1.7).sin()) * 0.6 + 3.0;
        let z = ((n as f32 * 2.3).cos()) * 0.6 - 3.0;
        let start = Transform::from_position(Vec3::new(x, 6.0, z)).with_scale(Vec3::splat(0.35));
        if self.balls.len() >= self.max {
            // reuse the oldest: teleport it back up and stop it
            let e = self.balls.remove(0);
            if let Some(mut t) = ctx.transform_mut(e) {
                *t = start;
            }
            if let Ok(mut rb) = ctx.world.get::<&mut goat_physics::RigidBody>(e) {
                rb.velocity = Vec3::ZERO;
            }
            self.balls.push(e);
            return;
        }
        let material = self.materials[n % self.materials.len()];
        let e = ctx.world.spawn((
            Name(format!("Ball {n}")),
            start,
            MeshRenderer::new(self.mesh, material),
            goat_physics::RigidBody::dynamic(),
            goat_physics::Collider::new(goat_physics::Shape::Sphere(0.5)).with_restitution(0.6),
            // a soft shadow under each falling ball
            goat_physics::BlobShadow { size: 1.1, ..Default::default() },
        ));
        self.balls.push(e);
    }
}

/// Turns to face the main camera (labels).
#[derive(serde::Serialize, serde::Deserialize)]
struct FaceCamera;

impl Behaviour for FaceCamera {
    fn late_update(&mut self, ctx: &mut Context, me: Entity) {
        let Some(cam) = ctx.find("Main Camera") else { return };
        let Ok(cam_pos) = ctx.world.get::<&Transform>(cam).map(|t| t.position) else { return };
        let parent_rot = ctx
            .world
            .get::<&Parent>(me)
            .ok()
            .and_then(|p| ctx.world.get::<&GlobalTransform>(p.0).ok().map(|g| g.0.to_scale_rotation_translation().1))
            .unwrap_or(Quat::IDENTITY);
        let Ok(me_pos) = ctx.world.get::<&GlobalTransform>(me).map(|g| g.0.w_axis.truncate()) else { return };
        // text faces +Z: point +Z at the camera, upright (yaw only)
        let to_cam = cam_pos - me_pos;
        let yaw = to_cam.x.atan2(to_cam.z);
        if let Some(mut t) = ctx.transform_mut(me) {
            t.rotation = parent_rot.inverse() * Quat::from_rotation_y(yaw);
        }
    }
}

/// Bobs up and down.
#[derive(serde::Serialize, serde::Deserialize)]
struct Bob {
    base: f32,
    phase: f32,
}

impl Behaviour for Bob {
    fn start(&mut self, ctx: &mut Context, me: Entity) {
        if let Some(t) = ctx.transform_mut(me) {
            self.base = t.position.y;
        }
    }
    fn update(&mut self, ctx: &mut Context, me: Entity) {
        let t = if FROZEN { 0.0 } else { ctx.time.elapsed };
        let y = self.base + (t * 2.0 + self.phase).sin() * 0.25;
        if let Some(mut t) = ctx.transform_mut(me) {
            t.position.y = y;
        }
    }
}

/// Orbits the camera around the center with the stick / D-pad; A toggles auto-rotation.
struct OrbitCamera {
    yaw: f32,
    pitch: f32,
    distance: f32,
    auto: bool,
}

impl Behaviour for OrbitCamera {
    fn update(&mut self, ctx: &mut Context, me: Entity) {
        let dt = delta(ctx);
        // input actions (input/controls.inputactions.json): stick or d-pad, L / R zoom, A, Start
        let (s, zoom, toggle, quit) = match goat_input::actions(ctx) {
            Some(a) => (a.read_vec2("Orbit"), a.read_f32("Zoom"), a.triggered("AutoOrbit"), a.triggered("Quit")),
            None => (ctx.input.stick(), 0.0, false, false),
        };
        if toggle {
            self.auto = !self.auto;
        }
        if let Some(on) = ctx.resources.get_mut::<ui_demo::Controls>().and_then(|c| c.auto_orbit.take()) {
            self.auto = on;
        }
        self.distance = (self.distance + zoom * 4.0 * dt).clamp(3.0, 14.0);
        if quit {
            ctx.quit();
        }
        self.yaw += (s.x * 1.8 + if self.auto { 0.35 } else { 0.0 }) * dt;
        self.pitch = (self.pitch + s.y * 1.2 * dt).clamp(-1.3, -0.08);
        let dir = Vec3::new(self.yaw.cos() * self.pitch.cos(), -self.pitch.sin(), self.yaw.sin() * self.pitch.cos());
        if let Some(mut t) = ctx.transform_mut(me) {
            t.position = Vec3::new(0.0, 0.6, 0.0) + dir * self.distance;
            t.look_at(Vec3::new(0.0, 0.6, 0.0), Vec3::Y);
        }
    }
}

impl Game for HelloGame {
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        plugins()
    }

    fn title(&self) -> &str {
        "GoatEngine — Hello"
    }

    fn start(&mut self, ctx: &mut Context) {
        info!("Hello from GoatEngine on {}", ctx.gfx.name());
        // a scene instead of the demo: `--scene scenes/navigation.scene.json` (desktop), or
        // GOAT_START_SCENE when building (the 3DS has no command line)
        let arg = std::env::args().skip_while(|a| a != "--scene").nth(1);
        if let Some(key) = arg.or(option_env!("GOAT_START_SCENE").map(str::to_string)).filter(|k| !k.is_empty()) {
            register(goat_scene::registry(ctx));
            match goat_scene::load_scene(ctx, &key, goat_scene::LoadMode::Single) {
                Ok(_) => return,
                Err(e) => goat_runtime::warn!("--scene {key}: {e}"),
            }
        }
        let gfx = &mut *ctx.gfx;

        // ground: mipmapped (no shimmering in the distance), 16-bit on the 3DS
        let checker = gfx
            .create_texture(&TextureData::checker(64, 8, Color::hex(0xd9dde6), Color::hex(0x8a93a6)).with_mipmaps().with_format(TextureFormat::Rgb565))
            .expect("texture");
        // orientation marker: top half red, bottom half white, a blue stripe on the left
        let marker_tex = {
            let mut t = TextureData::solid(Color::WHITE);
            (t.width, t.height) = (32, 32);
            t.rgba = (0..32 * 32)
                .flat_map(|i| {
                    let (x, y) = (i % 32, i / 32);
                    if x < 6 { [40, 90, 255, 255] } else if y < 16 { [230, 50, 50, 255] } else { [245, 245, 245, 255] }
                })
                .collect();
            gfx.create_texture(&t).expect("texture")
        };
        let crate_tex = gfx.create_texture(&TextureData::checker(32, 4, Color::hex(0xf2b53c), Color::hex(0xc9752a))).expect("texture");

        let cube = gfx.create_mesh(&MeshData::cube(1.0)).expect("mesh");
        let plane = gfx.create_mesh(&MeshData::plane(40.0, 20.0)).expect("mesh");
        let sphere = gfx.create_mesh(&MeshData::sphere(0.6, 16, 24)).expect("mesh");

        let ground = gfx.create_material(&MaterialDesc { texture: Some(checker), specular: Color::rgb(0.1, 0.1, 0.1), shininess: 8.0, ..Default::default() }).expect("material");
        let crate_mat = gfx.create_material(&MaterialDesc { texture: Some(crate_tex), specular: Color::rgb(0.35, 0.3, 0.25), shininess: 24.0, ..Default::default() }).expect("material");
        // the blue ball reflects the built-in sky cube map
        let sky = ctx.resources.get_mut::<goat_assets::AssetServer>().and_then(|a| a.cubemap(gfx, "builtin:sky").ok());
        let shiny = gfx.create_material(&MaterialDesc { base_color: Color::hex(0x3d8bff), specular: Color::rgb(0.9, 0.9, 0.9), shininess: 64.0, env_map: sky, reflectivity: 0.35, ..Default::default() }).expect("material");
        let marker = gfx.create_material(&MaterialDesc { texture: Some(marker_tex), lighting: goat_runtime::gfx::LightingModel::Unlit, ..Default::default() }).expect("material");
        let glass = gfx.create_material(&MaterialDesc { base_color: Color::rgba(0.4, 1.0, 0.6, 0.55), blend: BlendMode::Alpha, depth_write: false, ..Default::default() }).expect("material");

        // a lattice fence: a texture with round holes, cut out by the alpha test (dithered on desktop)
        let fence_tex = {
            let n = 64u32;
            let rgba: Vec<u8> = (0..n * n)
                .flat_map(|i| {
                    let (x, y) = ((i % n) as f32 % 16.0 - 7.5, (i / n) as f32 % 16.0 - 7.5);
                    let d = (x * x + y * y).sqrt();
                    let a = ((d - 4.5) / 2.0).clamp(0.0, 1.0);
                    [120, 84, 52, (a * 255.0) as u8]
                })
                .collect();
            gfx.create_texture(&TextureData { width: n, height: n, rgba, filter: goat_runtime::gfx::Filter::Linear, wrap: goat_runtime::gfx::Wrap::Repeat, mipmaps: false, format: TextureFormat::Rgba8, compressed: None }).expect("texture")
        };
        let fence = gfx.create_material(&MaterialDesc { texture: Some(fence_tex), alpha_cutoff: 0.5, alpha_dither: true, cull: goat_runtime::gfx::CullMode::None, specular: Color::BLACK, ..Default::default() }).expect("material");
        let world = &mut *ctx.world;
        let sky = Color::hex(0x1d2330);
        world.spawn((
            Name("Render Settings".into()),
            // hemisphere ambient: cool sky light from above, warm bounce from the ground
            RenderSettings { ambient: Color::rgb(0.26, 0.3, 0.4), ambient_ground: Some(Color::rgb(0.2, 0.15, 0.1)),
                // a little grading + glow (desktop: bloom / vignette too)
                post: Some(goat_runtime::gfx::PostFx { saturation: 1.15, contrast: 1.05, vignette: 0.35, bloom: 0.6, bloom_threshold: 0.75, ..Default::default() }),
                fog: Some(Fog { color: sky, start: 9.0, end: 24.0 }) },
        ));
        world.spawn((
            Name("Sun".into()),
            Transform::default().with_rotation(Quat::from_euler(goat_runtime::math::EulerRot::YXZ, 0.6, -0.9, 0.0)),
            DirectionalLight { intensity: if LIGHT_TEST { 0.0 } else { 0.75 }, ..Default::default() },
        ));
        world.spawn((
            Name("Lamp".into()),
            Transform::from_position(Vec3::new(-3.2, 1.0, -2.6)),
            PointLight { color: if LIGHT_TEST { Color::rgb(1.0, 0.0, 0.0) } else { Color::hex(0xff8a2a) }, intensity: 1.4, range: 4.5, ..Default::default() },
            Scripts::new().with(Bob { base: 0.0, phase: 0.7 }),
        ));
        let mut spot = Transform::from_position(Vec3::new(2.2, 4.5, -0.8));
        spot.look_at(Vec3::new(2.2, 0.0, -0.8), Vec3::Z);
        world.spawn((Name("Spot".into()), spot, SpotLight { color: if LIGHT_TEST { Color::rgb(0.0, 1.0, 0.0) } else { Color::hex(0x6ff0ff) }, intensity: 2.0, range: 8.0, spot_angle: 50.0, inner_spot_angle: 30.0, ..Default::default() }));
        world.spawn((
            Name("Marker".into()),
            Transform::from_position(Vec3::new(0.0, 0.5, -3.0)).with_scale(Vec3::new(1.6, 1.0, 0.1)),
            MeshRenderer::new(cube, marker),
        ));
        world.spawn((Name("Ground".into()), Transform::default(), MeshRenderer::new(plane, ground)));
        world.spawn((Name("Fence".into()), Transform::from_position(Vec3::new(-1.5, 0.6, 1.6)).with_scale(Vec3::new(2.4, 1.2, 0.04)), MeshRenderer::new(cube, fence)));
        world.spawn((
            Name("Crate".into()),
            Transform::from_position(Vec3::new(0.0, 0.7, 0.0)).with_scale(Vec3::splat(1.2)),
            MeshRenderer::new(cube, crate_mat),
            Scripts::new().with(Spinner { axis: Vec3::new(0.3, 1.0, 0.2).normalize(), speed: 0.9 }),
        ));
        world.spawn((
            Name("Ball".into()),
            Transform::from_position(Vec3::new(2.2, 0.8, -0.8)),
            MeshRenderer::new(sphere, shiny),
            goat_physics::BlobShadow { size: 1.3, opacity: 0.7, ..Default::default() },
            Scripts::new().with(Bob { base: 0.0, phase: 0.0 }),
        ));
        world.spawn((
            Name("Glass".into()),
            Transform::from_position(Vec3::new(-2.0, 0.6, 0.9)),
            MeshRenderer::new(cube, glass),
            Scripts::new().with(Spinner { axis: Vec3::Y, speed: -0.6 }).with(Bob { base: 0.0, phase: 1.6 }),
        ));
        for i in 0..8 {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            world.spawn((
                Name(format!("Pillar {i}")),
                Transform::from_position(Vec3::new(a.cos() * 4.5, 0.5, a.sin() * 4.5)).with_scale(Vec3::new(0.4, 1.0, 0.4)),
                MeshRenderer::new(cube, crate_mat),
            ));
        }
        // main camera (desktop window / 3DS top screen)
        world.spawn((
            Name("Main Camera".into()),
            Transform::from_position(Vec3::new(0.0, 3.0, 7.0)),
            Camera { clear: Some(sky), culling_mask: !layer_mask(&[hud::HUD_LAYER]), ..Default::default() },
            goat_audio::AudioListener,
            Scripts::new().with(OrbitCamera { yaw: 1.2, pitch: -0.42, distance: 7.0, auto: true }),
        ));
        // picture-in-picture on the main screen: a close-up of the crate
        let mut pip = Transform::from_position(Vec3::new(1.8, 1.6, 2.4));
        pip.look_at(Vec3::new(0.0, 0.7, 0.0), Vec3::Y);
        world.spawn((
            Name("PiP Camera".into()),
            pip,
            Camera {
                viewport: Viewport { x: 0.68, y: 0.05, width: 0.28, height: 0.32 },
                clear: Some(Color::hex(0x3a2f4a)),
                culling_mask: !layer_mask(&[hud::HUD_LAYER]),
                depth: 1,
                ..Default::default()
            },
        ));
        // second screen: a top-down map (3DS bottom screen)
        let mut map = Transform::from_position(Vec3::new(0.0, 12.0, 0.01));
        map.look_at(Vec3::ZERO, Vec3::NEG_Z);
        world.spawn((
            Name("Map Camera".into()),
            map,
            Camera {
                display: Display::Secondary,
                viewport: Viewport::FULL,
                projection: Projection::Orthographic { height: 11.0, near: 0.1, far: 50.0 },
                clear: Some(Color::hex(0x1d2330)),
                clear_depth: true,
                depth: 0,
                culling_mask: !layer_mask(&[hud::HUD_LAYER]),
            },
        ));
        hud::spawn(ctx);
        hud::spawn_texts(ctx);
        make_sounds(ctx);
        spawn_physics(ctx);
        spawn_animation_and_particles(ctx);
        ui_demo::spawn(ctx);

        // assets + prefabs: a pedestal with the logo texture, a glTF model (Blender's Suzanne)
        // spinning on it (behaviour stored in the prefab), its own light
        register(goat_scene::registry(ctx));
        match goat_scene::instantiate(ctx, "prefabs/shrine.prefab.json", None) {
            Ok(shrine) => {
                if let Some(mut t) = ctx.transform_mut(shrine) {
                    t.position = Vec3::new(-3.0, 0.0, 2.4);
                }
                // a world-space label above it, always facing the main camera
                ctx.world.spawn((
                    Name("Shrine Label".into()),
                    Transform::from_position(Vec3::new(0.0, 1.9, 0.0)),
                    Parent(shrine),
                    goat_text::Text::new("Đền <color=#ffffff>Dê</color> <i>Vàng</i>", hud::FONT, 0.26).with_align(goat_text::Align::Center, goat_text::VAlign::Bottom).with_color(Color::hex(0xffd36b)).with_outline(Color::hex(0x3a1d00), 0.05).with_shadow(Color::rgba(0.0, 0.0, 0.0, 0.5), [0.06, 0.08]).rich(),
                    Scripts::new().with(FaceCamera),
                ));
                // the shrine hums: a looping 3D sound, louder as the camera passes by
                ctx.world.spawn((Name("Shrine Hum".into()), Transform::from_position(Vec3::new(0.0, 1.0, 0.0)), Parent(shrine), goat_audio::AudioSource::new("drone").looping().play_on_start().spatial(1.5, 14.0).with_volume(0.5)));
            }
            Err(e) => goat_runtime::warn!("shrine: {e}"),
        }
        match goat_scene::instantiate(ctx, "prefabs/hud_logo.prefab.json", None) {
            Ok(logo) => {
                if let Some(mut t) = ctx.transform_mut(logo) {
                    t.position = hud::ORIGIN + Vec3::new(0.0, 70.0, 0.0);
                }
                // goat-tween: the logo breathes
                if !FROZEN {
                    goat_tween::tweens(ctx).add(goat_tween::Tween::scale_to(logo, Vec3::splat(1.08), 1.1).ease(goat_tween::Ease::InOutSine).loops(-1, goat_tween::LoopType::Yoyo));
                }
            }
            Err(e) => goat_runtime::warn!("hud logo: {e}"),
        }
        // a low wall of builtin cubes, merged into one draw with its lighting baked in
        if let (Ok(cube), Ok(mat)) = (goat_assets::mesh(ctx, "builtin:cube"), goat_assets::material(ctx, "materials/logo.mat.json")) {
            let wall = ctx.world.spawn((Name("Baked Wall".into()), Transform::from_position(Vec3::new(-4.5, 0.0, -1.5)), StaticBatching { bake_lighting: true }));
            for i in 0..6 {
                let h = 0.4 + (i % 3) as f32 * 0.25;
                ctx.world.spawn((Name(format!("Wall Block {i}")), Transform::from_position(Vec3::new(0.0, h * 0.5, i as f32 * 0.65)).with_scale(Vec3::new(0.6, h, 0.6)), Parent(wall), MeshRenderer::new(cube, mat)));
            }
        }
        // a cel-shaded ball: toon bands, rim light, black outline (materials/toon.mat.json)
        if let (Ok(sphere), Ok(mat)) = (goat_assets::mesh(ctx, "builtin:sphere"), goat_assets::material(ctx, "materials/toon.mat.json")) {
            ctx.world.spawn((Name("Toon Ball".into()), Transform::from_position(Vec3::new(0.9, 0.55, 2.2)).with_scale(Vec3::splat(1.1)), MeshRenderer::new(sphere, mat)));
        }
        // a JPEG texture and an ETC1 KTX (precompressed: the 3DS uploads its blocks as they are)
        if let (Ok(cube), Ok(photo), Ok(pattern)) = (goat_assets::mesh(ctx, "builtin:cube"), goat_assets::material(ctx, "materials/photo.mat.json"), goat_assets::material(ctx, "materials/pattern.mat.json")) {
            ctx.world.spawn((Name("JPEG Crate".into()), Transform::from_position(Vec3::new(2.3, 0.35, 2.9)).with_scale(Vec3::splat(0.7)), MeshRenderer::new(cube, photo)));
            ctx.world.spawn((Name("KTX Crate".into()), Transform::from_position(Vec3::new(3.2, 0.35, 2.3)).with_scale(Vec3::splat(0.7)), MeshRenderer::new(cube, pattern)));
        }
        // procedural materials (3DS: the proctex unit): a pond and a flame
        if let (Ok(plane), Ok(cube), Ok(water), Ok(fire)) = (goat_assets::mesh(ctx, "builtin:plane"), goat_assets::mesh(ctx, "builtin:cube"), goat_assets::material(ctx, "materials/water.mat.json"), goat_assets::material(ctx, "materials/fire.mat.json")) {
            ctx.world.spawn((Name("Pond".into()), Transform::from_position(Vec3::new(-2.6, 0.01, 3.4)).with_scale(Vec3::new(0.22, 1.0, 0.16)), MeshRenderer::new(plane, water)));
            ctx.world.spawn((Name("Flame".into()), Transform::from_position(Vec3::new(-2.6, 0.55, 3.4)).with_scale(Vec3::new(0.7, 1.1, 0.01)), MeshRenderer::new(cube, fire)));
        }
        // a firework made in Unity, exported by tools/unity-goat-exporter
        match goat_scene::instantiate(ctx, "fx/Firework.prefab.json", None) {
            Ok(fw) => {
                if let Some(mut t) = ctx.transform_mut(fw) {
                    t.position = Vec3::new(3.5, 0.2, 2.5);
                }
            }
            Err(e) => goat_runtime::warn!("firework: {e}"),
        }
    }
}

/// Physics: the ground and pillars collide, a stack of crates, balls raining on it.
/// Sounds made in code: a bonk (noise burst over a low thump) and a drone loop.
fn make_sounds(ctx: &mut Context) {
    use goat_audio::{Audio, AudioClip};
    let mut seed = 0x2545_f491u32;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    };
    let noise_samples: Vec<f32> = (0..4000).map(|_| noise()).collect();
    let bonk = AudioClip::synth(22050, 0.18, |t| {
        let n = noise_samples[((t * 22050.0) as usize).min(3999)];
        ((t * 140.0 * std::f32::consts::TAU).sin() * 0.7 + n * 0.3) * (-t * 28.0).exp()
    });
    let drone = AudioClip::synth(22050, 2.0, |t| {
        let w = |f: f32| (t * f * std::f32::consts::TAU).sin();
        (w(110.0) * 0.4 + w(165.0) * 0.25 + w(220.5) * 0.15) * (0.75 + 0.25 * w(0.5))
    });
    let audio = ctx.resources.get_or_default::<Audio>();
    audio.add_clip("bonk", bonk);
    audio.add_clip("drone", drone);
}

fn spawn_physics(ctx: &mut Context) {
    use goat_physics::{Collider, RigidBody, Shape};
    let (Ok(cube), Ok(sphere)) = (goat_assets::mesh(ctx, "builtin:cube"), goat_assets::mesh(ctx, "builtin:sphere")) else { return };
    let crate_mat = goat_assets::material(ctx, "materials/logo.mat.json").ok();
    let colors = [0xe8574a, 0x4ab3e8, 0x8be84a, 0xe8c64a];
    let materials: Vec<_> = colors
        .iter()
        .filter_map(|&c| ctx.gfx.create_material(&MaterialDesc { base_color: Color::hex(c), specular: Color::rgb(0.6, 0.6, 0.6), shininess: 48.0, ..Default::default() }).ok())
        .collect();
    let world = &mut *ctx.world;
    // ground: a thick box whose top is the visible plane (y = 0)
    world.spawn((Name("Ground Collider".into()), Transform::from_position(Vec3::new(0.0, -0.5, 0.0)).with_scale(Vec3::new(40.0, 1.0, 40.0)), Collider::unit_box()));
    // the pillars become static colliders
    let pillars: Vec<Entity> = world.query::<&Name>().iter().filter(|(_, n)| n.0.starts_with("Pillar")).map(|(e, _)| e).collect();
    for p in pillars {
        let _ = world.insert_one(p, Collider::unit_box());
    }
    // a stack of crates
    if let Some(mat) = crate_mat {
        for (i, (x, y)) in [(-0.55, 0.0), (0.55, 0.0), (0.0, 1.0)].into_iter().enumerate() {
            world.spawn((
                Name(format!("Stack Crate {i}")),
                Transform::from_position(Vec3::new(3.0 + x, 0.5 + y, -3.0)).with_scale(Vec3::splat(1.0)),
                MeshRenderer::new(cube, mat),
                RigidBody::dynamic(),
                Collider::unit_box().with_friction(0.8),
            ));
        }
    }
    let _ = Shape::Sphere(0.5);
    world.spawn((
        Name("Ball Spawner".into()),
        Transform::default(),
        Scripts::new().with(BallSpawner { mesh: sphere, materials, interval: 0.9, timer: 0.5, max: 14, balls: Vec::new() }),
    ));
}

/// Drives the tentacle's Animator Controller: "energy" breathes over time, X = poke (trigger),
/// holding Y = toggle sleep (bool).
struct TentacleMood {
    time: f32,
}

impl Behaviour for TentacleMood {
    fn update(&mut self, ctx: &mut Context, me: Entity) {
        self.time += delta(ctx);
        // X taps poke; holding Y for 0.4 s toggles sleep (a hold interaction)
        let (poke, sleep) = goat_input::actions(ctx).map(|a| (a.triggered("Poke"), a.triggered("Sleep"))).unwrap_or_default();
        if let Ok(mut a) = ctx.world.get::<&mut goat_animation::Animator>(me) {
            a.set_float("energy", 0.5 + 0.5 * (self.time * 0.7).sin());
            if poke {
                a.set_trigger("poke");
            }
            if sleep {
                let asleep = a.get_bool("asleep");
                a.set_bool("asleep", !asleep);
            }
        }
    }
}

/// An animated glTF (skinned tentacle swaying + a hopping cube), fire on the shrine, sparkles
/// over the ball rain.
fn spawn_animation_and_particles(ctx: &mut Context) {
    use goat_particles::{ParticleSystem, Range, Shape};
    match goat_assets::spawn_model(ctx, "models/tentacle.glb", None) {
        Ok(root) => {
            if let Some(mut t) = ctx.transform_mut(root) {
                *t = Transform::from_position(Vec3::new(-4.6, 0.0, -1.2)).with_scale(Vec3::splat(0.9));
            }
            // the Animator Controller: Idle blend tree (Sway ↔ Hop by "energy"), X pokes, Y sleeps
            let anim = ctx
                .resources
                .get::<goat_assets::AssetServer>()
                .map(|a| goat_animation::Animator::from_controller_asset(a, "animators/tentacle.controller.json"))
                .unwrap_or_else(|| Err("no asset server".into()))
                .unwrap_or_else(|e| {
                    goat_runtime::warn!("tentacle controller: {e}");
                    goat_animation::Animator::playing("Sway")
                });
            let _ = ctx.world.insert(root, (anim, Scripts::new().with(TentacleMood { time: 0.0 })));
        }
        Err(e) => goat_runtime::warn!("tentacle: {e}"),
    }
    let fire = ParticleSystem {
        rate: 45.0,
        shape: Shape::Cone { angle: 12.0, radius: 0.08 },
        lifetime: Range(0.45, 0.8),
        speed: Range(0.6, 1.2),
        size: Range(0.22, 0.38),
        color_start: Color::rgba(1.0, 0.62, 0.22, 0.9),
        color_end: Color::rgba(0.9, 0.12, 0.02, 0.0),
        size_over_life: (1.0, 0.25),
        gravity: -0.15,
        ..Default::default()
    };
    for (i, x) in [-0.75f32, 0.75].into_iter().enumerate() {
        ctx.world.spawn((Name(format!("Torch {i}")), Transform::from_position(Vec3::new(-3.0 + x, 0.62, 2.4 + 0.75)), fire.clone().with_seed(7 + i as u32)));
    }
    ctx.world.spawn((
        Name("Sparkles".into()),
        Transform::from_position(Vec3::new(3.0, 5.0, -3.0)),
        ParticleSystem {
            rate: 14.0,
            shape: Shape::Sphere { radius: 1.2 },
            lifetime: Range(1.0, 1.8),
            speed: Range(0.1, 0.4),
            size: Range(0.08, 0.16),
            color_start: Color::rgba(0.55, 0.95, 1.0, 1.0),
            color_end: Color::rgba(0.3, 0.5, 1.0, 0.0),
            gravity: 0.08,
            drag: 0.5,
            ..Default::default()
        }
        .with_seed(99),
    ));
}
